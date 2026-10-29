use super::*;
use soyeht_protocol::data_directory::data_directory;
use soyeht_service::{CommandOutput, System};
use std::{cell::RefCell, fs};

/// A service manager that says yes to everything and remembers what it was asked.
#[derive(Default)]
struct Yes(RefCell<Vec<String>>);

impl Commands for Yes {
    fn run(&self, program: &str, arguments: &[&str]) -> soyeht_service::Result<CommandOutput> {
        self.0
            .borrow_mut()
            .push(format!("{program} {}", arguments.join(" ")));
        Ok(CommandOutput {
            success: true,
            ..CommandOutput::default()
        })
    }
}

struct Rig {
    root: PathBuf,
    layout: Layout,
}

impl Rig {
    fn new(label: &str) -> Self {
        // Short enough for the path of a Unix socket, which the temporary folder of macOS is not.
        let parent = if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        let root = parent.join(format!("next-uninstall-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let layout = Layout {
            system: System::current(),
            profile: &soyeht_protocol::profile::PROD,
            data: data_directory(&soyeht_protocol::profile::PROD, Some(root.join("base"))).unwrap(),
            home: root.join("home"),
            config_home: None,
            user_id: "501".into(),
            user_name: None,
        };
        fs::create_dir_all(layout.data.join("logs")).unwrap();
        fs::write(layout.data.join("soyeht.db"), "rows").unwrap();
        fs::create_dir_all(&layout.home).unwrap();
        Self { root, layout }
    }
}

impl Rig {
    fn run(
        &self,
        commands: &Yes,
        options: &Options,
        ask: impl FnOnce(&Layout) -> Result<String>,
    ) -> Result<Value> {
        uninstall(
            &self.layout,
            commands,
            options,
            (&self.root.join("base"), Duration::ZERO),
            ask,
        )
    }
}

impl Rig {
    /// The message of a run that must be refused.
    fn refusal(&self, commands: &Yes, options: &Options) -> String {
        self.run(commands, options, no_question)
            .unwrap_err()
            .to_string()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn no_question(_: &Layout) -> Result<String> {
    Err(invalid("asked a question"))
}

#[test]
fn the_options_are_two_flags_and_yes_needs_delete_data() {
    assert_eq!(parse(&[]).unwrap(), Options::default());
    assert_eq!(
        parse(&["--delete-data", "--yes"]).unwrap(),
        Options {
            delete_data: true,
            yes: true
        }
    );
    assert!(parse(&["--yes"]).is_err());
    assert!(parse(&["--purge"]).is_err());
}

#[test]
fn only_the_profile_name_confirms() {
    assert!(confirms("prod\n", "prod"));
    assert!(confirms("  prod ", "prod"));
    for answer in ["", "y", "yes", "dev", "PROD", "prod prod"] {
        assert!(!confirms(answer, "prod"), "{answer:?}");
    }
}

const DELETE: Options = Options {
    delete_data: true,
    yes: false,
};
const DELETE_YES: Options = Options {
    delete_data: true,
    yes: true,
};

#[test]
fn the_default_unregisters_both_services_and_keeps_the_data() {
    let rig = Rig::new("keep");
    let commands = Yes::default();
    let result = rig
        .run(&commands, &Options::default(), no_question)
        .unwrap();
    assert_eq!(result["dataDeleted"], json!(false));
    assert_eq!(result["services"].as_array().unwrap().len(), 2);
    assert!(rig.layout.data.join("soyeht.db").is_file());
    // Only the names of this profile were asked about, never a legacy one.
    let asked = commands.0.borrow().join("\n");
    assert!(asked.contains("next"), "{asked}");
    for legacy in soyeht_protocol::legacy::BUNDLE_IDS {
        assert!(!asked.contains(legacy), "{asked}");
    }
}

#[test]
fn delete_data_asks_first_and_a_wrong_answer_changes_nothing() {
    let rig = Rig::new("refuse");
    let commands = Yes::default();
    let error = rig
        .run(&commands, &DELETE, |_| Ok("yes\n".into()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("nothing was changed"), "{error}");
    assert!(commands.0.borrow().is_empty(), "no service was touched");
    assert!(rig.layout.data.join("soyeht.db").is_file());
}

#[test]
fn delete_data_with_the_profile_name_removes_the_data_after_the_services() {
    let rig = Rig::new("delete");
    let commands = Yes::default();
    let result = rig
        .run(&commands, &DELETE, |_| Ok("prod\n".into()))
        .unwrap();
    assert_eq!(result["dataDeleted"], json!(true));
    assert!(!rig.layout.data.exists());
    assert!(!commands.0.borrow().is_empty());
}

#[test]
fn yes_deletes_without_asking() {
    let rig = Rig::new("yes");
    let result = rig.run(&Yes::default(), &DELETE_YES, no_question).unwrap();
    assert_eq!(result["dataDeleted"], json!(true));
    assert!(!rig.layout.data.exists());
}

#[test]
fn a_refused_deletion_leaves_the_services_registered_and_asks_nothing() {
    for name in ["nextcloud", "Documents/nextjs-app", "next-notes"] {
        let mut rig = Rig::new("guard");
        let place = rig.root.join("base").join(name);
        fs::create_dir_all(&place).unwrap();
        fs::write(place.join("mine"), "x").unwrap();
        rig.layout.data = place.clone();
        let commands = Yes::default();
        let error = rig.refusal(&commands, &DELETE_YES);
        assert!(error.contains("nothing was deleted"), "{name}: {error}");
        assert!(
            commands.0.borrow().is_empty(),
            "{name}: a service was touched"
        );
        assert!(place.join("mine").is_file(), "{name}");
        // Nor does the prompt come first for a place that would be refused.
        let asked = rig
            .run(&commands, &DELETE, no_question)
            .unwrap_err()
            .to_string();
        assert!(!asked.contains("asked a question"), "{name}: {asked}");
    }
}

#[cfg(unix)]
#[test]
fn programs_that_do_not_stop_keep_the_data_and_the_message_says_what_happened() {
    let rig = Rig::new("running");
    let _engine = std::os::unix::net::UnixListener::bind(
        rig.layout
            .data
            .join(soyeht_protocol::profile::ENGINE_SOCKET),
    )
    .unwrap();
    let commands = Yes::default();
    let error = rig.refusal(&commands, &DELETE_YES);
    assert!(error.contains("services are unregistered"), "{error}");
    assert!(!error.contains("soyeht service stop"), "{error}");
    assert!(rig.layout.data.join("soyeht.db").is_file());
    assert!(
        !commands.0.borrow().is_empty(),
        "the services were unregistered"
    );
}

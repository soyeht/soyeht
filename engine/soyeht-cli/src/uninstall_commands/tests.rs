use super::*;
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
        let root =
            std::env::temp_dir().join(format!("next-uninstall-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let layout = Layout {
            system: System::current(),
            profile: &soyeht_protocol::profile::PROD,
            data: root.join("data/next"),
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

#[test]
fn the_default_unregisters_both_services_and_keeps_the_data() {
    let rig = Rig::new("keep");
    let commands = Yes::default();
    let result = uninstall(
        &rig.layout,
        &commands,
        &Options::default(),
        &rig.root,
        no_question,
    )
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
    let options = Options {
        delete_data: true,
        yes: false,
    };
    let error = uninstall(&rig.layout, &commands, &options, &rig.root, |_| {
        Ok("yes\n".into())
    })
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
    let options = Options {
        delete_data: true,
        yes: false,
    };
    let result = uninstall(&rig.layout, &commands, &options, &rig.root, |_| {
        Ok("prod\n".into())
    })
    .unwrap();
    assert_eq!(result["dataDeleted"], json!(true));
    assert!(!rig.layout.data.exists());
    assert!(!commands.0.borrow().is_empty());
}

#[test]
fn yes_deletes_without_asking() {
    let rig = Rig::new("yes");
    let options = Options {
        delete_data: true,
        yes: true,
    };
    let result = uninstall(
        &rig.layout,
        &Yes::default(),
        &options,
        &rig.root,
        no_question,
    )
    .unwrap();
    assert_eq!(result["dataDeleted"], json!(true));
    assert!(!rig.layout.data.exists());
}

#[test]
fn a_place_that_is_not_the_apps_is_not_deleted_even_when_confirmed() {
    let mut rig = Rig::new("guard");
    let plain = std::env::temp_dir().join(format!("plain-folder-cli-{}", std::process::id()));
    fs::create_dir_all(&plain).unwrap();
    fs::write(plain.join("mine"), "x").unwrap();
    rig.layout.data = plain.clone();
    let options = Options {
        delete_data: true,
        yes: true,
    };
    let refused = uninstall(
        &rig.layout,
        &Yes::default(),
        &options,
        &rig.root,
        no_question,
    );
    let kept = plain.join("mine").is_file();
    fs::remove_dir_all(&plain).unwrap();
    assert!(refused.is_err());
    assert!(kept);
}

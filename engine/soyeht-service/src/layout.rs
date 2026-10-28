//! Where a profile's service lives on this machine, and the definition of each component.

use crate::{
    definition::Definition,
    error::{Error, Result},
    guard, host, policy,
    system::{Component, System, service_name},
};
use soyeht_protocol::profile::Profile;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// The system directories a program started by launchd finds its tools in, before the user's
/// own. The Mac's service manager starts a program with a minimal `PATH`.
const MACOS_SYSTEM_PATH: [&str; 6] = [
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];
const LINUX_SYSTEM_PATH: [&str; 3] = ["/usr/local/bin", "/usr/bin", "/bin"];
/// The user's own tool folders, below the home folder, searched after the system's.
const USER_TOOL_FOLDERS: [&str; 2] = [".local/bin", ".cargo/bin"];

/// A profile's service on one system: the facts about the machine that the definitions and the
/// commands need.
#[derive(Clone, Debug)]
pub struct Layout {
    pub system: System,
    pub profile: &'static Profile,
    /// The profile's data directory: the socket, the database and the logs.
    pub data: PathBuf,
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME` when it is set and absolute: where systemd looks for user units.
    pub config_home: Option<PathBuf>,
    /// The numeric user id (macOS, Linux) or the security identifier (Windows).
    pub user_id: String,
    pub user_name: Option<String>,
}

impl Layout {
    /// The layout of `profile` for the user running this program, with the data in `data`.
    pub fn current(profile: &'static Profile, data: PathBuf) -> Result<Self> {
        Ok(Self {
            system: System::current(),
            profile,
            data,
            home: std::env::home_dir()
                .filter(|home| home.is_absolute())
                .ok_or(Error::Environment("the home folder"))?,
            config_home: std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute()),
            user_id: soyeht_platform::user::id()?,
            // Not set, or not text: the agent then goes without `USER`.
            user_name: match std::env::var("USER") {
                Ok(name) if !name.is_empty() => Some(name),
                _ => None,
            },
        })
    }

    /// The name the system knows `component` by.
    pub fn name(&self, component: Component) -> &'static str {
        service_name(self.profile, self.system, component)
    }

    /// The file the supervisor and the engine append their output to, where the manager can do
    /// that: `logs/<component>.log` in the data directory, which a program the app starts
    /// itself writes to as well (`soyeht_platform::process::detached::logs_directory`).
    pub fn log(&self, component: Component) -> PathBuf {
        soyeht_platform::process::detached::logs_directory(&self.data)
            .join(format!("{}.log", component.name()))
    }

    /// The definition of `component` for programs found in `programs`.
    pub fn definition(&self, component: Component, programs: &Path) -> Result<Definition> {
        guard::require_path("data directory", &self.data)?;
        guard::require_programs("program folder", programs)?;
        let text = |path: &Path, what: &'static str| {
            path.to_str()
                .map(str::to_owned)
                .ok_or(Error::Unrepresentable(what))
        };
        let program = programs.join(format!(
            "{}{}",
            component.program(),
            self.system.executable_suffix()
        ));
        let mut arguments = Vec::new();
        if component == Component::Engine {
            // A scheduled task cannot set `SOYEHT_PROFILE`, so the profile is an argument.
            arguments.extend(["--profile".to_owned(), self.profile.name.to_owned()]);
        }
        arguments.extend(["--data-dir".to_owned(), text(&self.data, "data directory")?]);
        let mut definition = Definition {
            name: self.name(component).to_owned(),
            description: component.description().to_owned(),
            files: vec![text(&program, "program folder")?],
            program: text(&program, "program folder")?,
            arguments,
            environment: self.environment()?,
            log: Some(text(&self.log(component), "data directory")?),
            after: (component == Component::Engine && self.system == System::Linux)
                .then(|| self.name(Component::Ptyd).to_owned()),
            user: (self.system == System::Windows).then(|| self.user_id.clone()),
        };
        if self.system == System::Windows {
            definition = self.through_host(definition, programs)?;
        }
        definition.check()?;
        Ok(definition)
    }

    /// The definition of a Windows task: the task starts the host, which starts the program with
    /// no window, its output in the log, and its exit code as its own (`host`).
    fn through_host(&self, definition: Definition, programs: &Path) -> Result<Definition> {
        let host = programs.join(format!(
            "{}{}",
            host::PROGRAM,
            self.system.executable_suffix()
        ));
        let host = host
            .to_str()
            .ok_or(Error::Unrepresentable("program folder"))?
            .to_owned();
        let log = definition.log.clone().unwrap_or_default();
        let mut arguments = vec![
            host::LOG_OPTION.to_owned(),
            log,
            host::PROGRAM_SEPARATOR.to_owned(),
            definition.program.clone(),
        ];
        arguments.extend(definition.arguments);
        let mut files = vec![host.clone()];
        files.extend(definition.files);
        Ok(Definition {
            program: host,
            arguments,
            files,
            ..definition
        })
    }

    /// The variables the programs start with. launchd and systemd start a program with a
    /// minimal environment, so the tools the harnesses call (`git`, `node`, the user's own) are
    /// named here; a Windows task inherits the user's.
    fn environment(&self) -> Result<BTreeMap<String, String>> {
        let system_path: &[&str] = match self.system {
            System::Macos => &MACOS_SYSTEM_PATH,
            System::Linux => &LINUX_SYSTEM_PATH,
            System::Windows => return Ok(BTreeMap::new()),
        };
        let mut folders: Vec<PathBuf> = system_path.iter().map(PathBuf::from).collect();
        folders.extend(
            USER_TOOL_FOLDERS
                .iter()
                .map(|folder| self.home.join(folder)),
        );
        let path = std::env::join_paths(&folders)
            .map_err(|_| Error::Unrepresentable("home folder"))?
            .into_string()
            .map_err(|_| Error::Unrepresentable("home folder"))?;
        let mut environment = BTreeMap::from([("PATH".to_owned(), path)]);
        if self.system == System::Macos {
            environment.insert(
                "HOME".into(),
                self.home
                    .to_str()
                    .ok_or(Error::Unrepresentable("home folder"))?
                    .to_owned(),
            );
            if let Some(user) = &self.user_name {
                environment.insert("USER".into(), user.clone());
            }
        }
        Ok(environment)
    }

    /// The file the system keeps the definition of `component` in, when it is a file we write.
    /// On Windows it is the copy of the task we hand to `schtasks`.
    pub(crate) fn definition_file(&self, component: Component) -> PathBuf {
        let name = self.name(component);
        match self.system {
            System::Macos => self
                .home
                .join("Library/LaunchAgents")
                .join(format!("{name}.plist")),
            System::Linux => self
                .config_home
                .clone()
                .unwrap_or_else(|| self.home.join(".config"))
                .join("systemd/user")
                .join(name),
            System::Windows => self
                .data
                .join(policy::TASK_COPY_FOLDER)
                .join(format!("{name}.xml")),
        }
    }
}

#[cfg(test)]
pub(crate) fn test_layout(system: System, root: &Path) -> Layout {
    use soyeht_protocol::profile::PROD;
    Layout {
        system,
        profile: &PROD,
        data: root.join("data/next"),
        home: root.join("home"),
        config_home: None,
        user_id: if system == System::Windows {
            "S-1-5-21-1-2-3-1001".into()
        } else {
            "501".into()
        },
        user_name: Some("alex".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/tmp/next-test";

    fn programs() -> PathBuf {
        PathBuf::from("/opt/next/bin")
    }

    #[test]
    fn the_engine_names_its_profile_and_the_supervisor_does_not() {
        let layout = test_layout(System::Macos, Path::new(ROOT));
        let engine = layout.definition(Component::Engine, &programs()).unwrap();
        assert_eq!(engine.name, layout.profile.engine_launchd_label);
        assert_eq!(engine.program, "/opt/next/bin/soyeht-engine");
        assert_eq!(
            engine.arguments,
            [
                "--profile",
                "prod",
                "--data-dir",
                "/tmp/next-test/data/next"
            ]
        );
        let ptyd = layout.definition(Component::Ptyd, &programs()).unwrap();
        assert_eq!(ptyd.arguments, ["--data-dir", "/tmp/next-test/data/next"]);
        assert_eq!(
            ptyd.log.as_deref(),
            Some("/tmp/next-test/data/next/logs/ptyd.log")
        );
    }

    #[test]
    fn launchd_and_systemd_get_a_path_to_the_tools_and_a_windows_task_gets_nothing() {
        let mac = test_layout(System::Macos, Path::new(ROOT))
            .definition(Component::Engine, &programs())
            .unwrap();
        assert_eq!(
            mac.environment["PATH"],
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:\
/tmp/next-test/home/.local/bin:/tmp/next-test/home/.cargo/bin"
        );
        assert_eq!(mac.environment["HOME"], "/tmp/next-test/home");
        assert_eq!(mac.environment["USER"], "alex");
        let linux = test_layout(System::Linux, Path::new(ROOT))
            .definition(Component::Engine, &programs())
            .unwrap();
        assert_eq!(
            linux.environment.keys().collect::<Vec<_>>(),
            ["PATH"],
            "systemd sets HOME and USER itself"
        );
        let windows = test_layout(System::Windows, Path::new(ROOT))
            .definition(Component::Engine, &programs())
            .unwrap();
        assert!(windows.environment.is_empty());
    }

    #[test]
    fn only_the_linux_engine_waits_for_the_supervisor_and_only_a_task_names_a_user() {
        let engine = |system| {
            test_layout(system, Path::new(ROOT))
                .definition(Component::Engine, &programs())
                .unwrap()
        };
        let linux = engine(System::Linux);
        assert_eq!(
            linux.after.as_deref(),
            Some(linux.name.replace("engine", "ptyd").as_str())
        );
        assert_eq!(engine(System::Macos).after, None);
        assert_eq!(
            engine(System::Windows).user.as_deref(),
            Some("S-1-5-21-1-2-3-1001")
        );
        assert_eq!(engine(System::Macos).user, None);
    }

    #[test]
    fn a_windows_task_starts_the_host_which_starts_the_program_and_keeps_its_log() {
        let layout = test_layout(System::Windows, Path::new(ROOT));
        let definition = layout.definition(Component::Ptyd, &programs()).unwrap();
        let log = "/tmp/next-test/data/next/logs/ptyd.log";
        assert_eq!(definition.program, "/opt/next/bin/soyeht-service-host.exe");
        assert_eq!(
            definition.arguments,
            [
                "--log",
                log,
                "--",
                "/opt/next/bin/soyeht-ptyd.exe",
                "--data-dir",
                "/tmp/next-test/data/next"
            ]
        );
        assert_eq!(definition.log.as_deref(), Some(log));
        assert_eq!(
            definition.files,
            [
                "/opt/next/bin/soyeht-service-host.exe",
                "/opt/next/bin/soyeht-ptyd.exe"
            ]
        );
        // The other systems start the program itself.
        let mac = test_layout(System::Macos, Path::new(ROOT))
            .definition(Component::Ptyd, &programs())
            .unwrap();
        assert_eq!(mac.program, "/opt/next/bin/soyeht-ptyd");
        assert_eq!(mac.files, ["/opt/next/bin/soyeht-ptyd"]);
    }

    #[test]
    fn the_definition_files_are_where_each_system_looks() {
        let root = Path::new(ROOT);
        let file = |system| test_layout(system, root).definition_file(Component::Engine);
        let profile = test_layout(System::Macos, root).profile;
        assert_eq!(
            file(System::Macos),
            root.join("home/Library/LaunchAgents")
                .join(format!("{}.plist", profile.engine_launchd_label))
        );
        assert_eq!(
            file(System::Linux),
            root.join("home/.config/systemd/user")
                .join(profile.engine_systemd_unit)
        );
        assert_eq!(
            file(System::Windows),
            root.join("data/next/service")
                .join(format!("{}.xml", profile.engine_windows_task))
        );
        let mut layout = test_layout(System::Linux, root);
        layout.config_home = Some(root.join("xdg"));
        assert_eq!(
            layout.definition_file(Component::Ptyd),
            root.join("xdg/systemd/user")
                .join(profile.ptyd_systemd_unit)
        );
    }

    #[test]
    fn a_data_directory_of_the_legacy_app_is_refused() {
        let mut layout = test_layout(System::Macos, Path::new(ROOT));
        layout.data = guard::legacy_values::data(Path::new("/Users/a"));
        assert!(layout.definition(Component::Engine, &programs()).is_err());
        layout.data = PathBuf::from("/Users/a/Library/Application Support/Next/service");
        assert!(layout.definition(Component::Engine, &programs()).is_ok());
        assert!(
            layout
                .definition(Component::Engine, &guard::legacy_values::program())
                .is_err()
        );
    }

    #[test]
    fn programs_may_sit_in_an_ordinary_install_folder() {
        let layout = test_layout(System::Linux, Path::new(ROOT));
        for folder in ["/usr/bin", "/usr/local/bin", "/opt/soyeht"] {
            let definition = layout
                .definition(Component::Engine, Path::new(folder))
                .unwrap();
            assert_eq!(definition.program, format!("{folder}/soyeht-engine"));
        }
    }

    #[test]
    fn a_relative_or_dotted_data_directory_or_program_folder_is_refused() {
        let layout = test_layout(System::Linux, Path::new(ROOT));
        for folder in ["bin", "/opt/next/../bin", "/opt/./bin"] {
            assert!(
                layout
                    .definition(Component::Engine, Path::new(folder))
                    .is_err()
            );
        }
        for data in ["next/data", "/tmp/next/../x", "/tmp/next/./x"] {
            let mut layout = layout.clone();
            layout.data = PathBuf::from(data);
            assert!(
                layout.definition(Component::Engine, &programs()).is_err(),
                "{data}"
            );
        }
    }
}

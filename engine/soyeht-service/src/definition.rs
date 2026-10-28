use crate::{
    error::{Error, Result},
    guard,
};
use std::collections::BTreeMap;

/// Everything a system's service file says, independent of the system.
///
/// A definition is plain text: the system's files cannot hold a path that is not text or a value
/// with a control character, so [`Definition::check`] refuses those before anything is rendered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    /// The name the system knows the service by: a launchd label, a systemd unit file name or a
    /// Task Scheduler task name. It carries "next".
    pub name: String,
    pub description: String,
    /// The absolute path of the program the system starts.
    pub program: String,
    pub arguments: Vec<String>,
    /// Variables the program starts with. Task Scheduler cannot set any, so a definition for
    /// it keeps these empty and passes what the program needs on its command line.
    pub environment: BTreeMap<String, String>,
    /// The file the program's standard output and error are appended to. launchd and systemd do
    /// it themselves; for Task Scheduler the host program does (`host`), and the task file does
    /// not mention it.
    pub log: Option<String>,
    /// Every program file the service needs to exist (the program the system starts, and the
    /// one the Windows host starts for it); an install is refused when one is missing.
    pub files: Vec<String>,
    /// The name of a service this one starts after (systemd), so the supervisor is up first.
    pub after: Option<String>,
    /// The account a Windows task belongs to, as a security identifier.
    pub user: Option<String>,
}

impl Definition {
    /// Refuses a definition that cannot be written faithfully or could name the legacy app.
    pub fn check(&self) -> Result<()> {
        guard::require_name("service name", &self.name)?;
        if let Some(after) = &self.after {
            guard::require_name("service this one follows", after)?;
        }
        let texts = [
            ("service name", &self.name),
            ("description", &self.description),
            ("program", &self.program),
        ];
        for (what, text) in texts {
            plain(what, text)?;
        }
        for argument in &self.arguments {
            plain("argument", argument)?;
        }
        for (name, value) in &self.environment {
            plain("variable name", name)?;
            plain("variable value", value)?;
            if name.is_empty() || name.contains('=') {
                return Err(Error::Unrepresentable("variable name"));
            }
        }
        for text in [&self.log, &self.after, &self.user].into_iter().flatten() {
            plain("service setting", text)?;
        }
        for file in &self.files {
            plain("program file", file)?;
        }
        if self.program.is_empty() {
            return Err(Error::Unrepresentable("program"));
        }
        Ok(())
    }
}

fn plain(what: &'static str, text: &str) -> Result<()> {
    if text.chars().any(char::is_control) {
        return Err(Error::Unrepresentable(what));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn sample(name: &str) -> Definition {
    Definition {
        name: name.to_owned(),
        description: "Soyeht engine".into(),
        program: "/opt/next/soyeht-engine".into(),
        arguments: vec!["--data-dir".into(), "/data/next".into()],
        environment: BTreeMap::from([("PATH".into(), "/usr/bin".into())]),
        log: Some("/data/next/logs/engine.log".into()),
        files: vec!["/opt/next/soyeht-engine".into()],
        after: None,
        user: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_definition_passes() {
        sample("com.soyeht.next.test.engine").check().unwrap();
    }

    #[test]
    fn a_legacy_name_is_refused() {
        assert!(sample(&guard::legacy_values::name()).check().is_err());
        assert!(sample("com.soyeht.engine").check().is_err());
        let mut definition = sample("com.soyeht.next.test.engine");
        definition.after = Some(guard::legacy_values::name());
        assert!(definition.check().is_err());
    }

    #[test]
    fn a_value_with_a_control_character_is_refused() {
        let name = "com.soyeht.next.test.engine";
        type Spoil = fn(&mut Definition);
        let spoils: [Spoil; 6] = [
            |d| d.description = "two\nlines".into(),
            |d| d.program = "/a\tb".into(),
            |d| d.arguments.push("--x\r".into()),
            |d| {
                d.environment.insert("K".into(), "v\0".into());
            },
            |d| {
                d.environment.insert("A=B".into(), "v".into());
            },
            |d| d.log = Some("/log\n".into()),
        ];
        for spoil in spoils {
            let mut definition = sample(name);
            spoil(&mut definition);
            assert!(definition.check().is_err());
        }
    }
}

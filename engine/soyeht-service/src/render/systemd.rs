//! A systemd user unit (`~/.config/systemd/user/<unit>`), started with `systemctl --user`.
//!
//! `Restart=always` restarts the program however it ended, `RestartSec` waits the policy's
//! delay, and `StartLimitIntervalSec=0` removes the rate limit that would otherwise leave a
//! unit "failed" for good after a few quick restarts. `WantedBy=default.target` starts it when
//! the user's manager starts, which is at login. The engine `Wants=` and starts `After=` the
//! supervisor, but does not `Require` it: restarting the supervisor must not restart the engine.
//! `ConditionPathExists=` the program skips the start, without a failure and so without the
//! one-second restart, when the program has been removed or moved (a package removed, a folder
//! deleted) while the unit file stays in the person's home (ADR 0121).

use crate::{definition::Definition, policy};

/// `text` as one word of an `ExecStart=` line: quoted, with the characters systemd would
/// otherwise read (`\`, `"`, the `%` specifiers and the `$` variables) escaped.
fn exec_word(text: &str) -> String {
    format!("\"{}\"", quoted(text).replace('$', "$$"))
}

/// `text` inside double quotes in a unit file, where `%` starts a specifier.
fn quoted(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
}

pub(super) fn render(definition: &Definition) -> String {
    let mut text = format!(
        "[Unit]\nDescription={}\n",
        definition.description.replace('%', "%%")
    );
    text.push_str(&format!(
        "ConditionPathExists={}\n",
        definition.program.replace('%', "%%")
    ));
    if let Some(after) = &definition.after {
        text.push_str(&format!("Wants={after}\nAfter={after}\n"));
    }
    text.push_str("StartLimitIntervalSec=0\n\n[Service]\nType=simple\n");
    let words: Vec<String> = std::iter::once(&definition.program)
        .chain(&definition.arguments)
        .map(|word| exec_word(word))
        .collect();
    text.push_str(&format!("ExecStart={}\n", words.join(" ")));
    for (name, value) in &definition.environment {
        text.push_str(&format!(
            "Environment=\"{}\"\n",
            quoted(&format!("{name}={value}"))
        ));
    }
    text.push_str(&format!(
        "Restart=always\nRestartSec={}\n",
        policy::RESTART_DELAY_SECONDS
    ));
    if let Some(log) = &definition.log {
        let log = log.replace('%', "%%");
        text.push_str(&format!(
            "StandardOutput=append:{log}\nStandardError=append:{log}\n"
        ));
    }
    text.push_str("\n[Install]\nWantedBy=default.target\n");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::sample;

    #[test]
    fn the_unit_starts_at_login_and_restarts_however_it_ended() {
        let mut definition = sample("soyeht-next-test-engine.service");
        definition.after = Some("soyeht-next-test-ptyd.service".into());
        assert_eq!(
            render(&definition),
            "[Unit]\nDescription=Soyeht engine\n\
ConditionPathExists=/opt/next/soyeht-engine\n\
Wants=soyeht-next-test-ptyd.service\nAfter=soyeht-next-test-ptyd.service\n\
StartLimitIntervalSec=0\n\n\
[Service]\nType=simple\n\
ExecStart=\"/opt/next/soyeht-engine\" \"--data-dir\" \"/data/next\"\n\
Environment=\"PATH=/usr/bin\"\n\
Restart=always\nRestartSec=1\n\
StandardOutput=append:/data/next/logs/engine.log\n\
StandardError=append:/data/next/logs/engine.log\n\n\
[Install]\nWantedBy=default.target\n"
        );
    }

    #[test]
    fn a_supervisor_unit_waits_for_nothing_and_logs_nowhere_without_a_log() {
        let mut definition = sample("soyeht-next-test-ptyd.service");
        definition.environment.clear();
        definition.log = None;
        let text = render(&definition);
        assert!(!text.contains("After="));
        assert!(!text.contains("Environment="));
        assert!(!text.contains("StandardOutput"));
    }

    #[test]
    fn systemd_syntax_in_a_value_stays_literal() {
        let mut definition = sample("soyeht-next-test-engine.service");
        definition.program = "/My Apps/50%/next \"x\"/soyeht-engine".into();
        definition.arguments = vec!["$HOME and \\n".into()];
        definition
            .environment
            .insert("GREETING".into(), "100% \"quoted\" $x".into());
        let text = render(&definition);
        assert!(text.contains(
            "ExecStart=\"/My Apps/50%%/next \\\"x\\\"/soyeht-engine\" \"$$HOME and \\\\n\"\n"
        ));
        assert!(text.contains("Environment=\"GREETING=100%% \\\"quoted\\\" $x\"\n"));
        assert!(text.contains("ConditionPathExists=/My Apps/50%%/next \"x\"/soyeht-engine\n"));
    }
}

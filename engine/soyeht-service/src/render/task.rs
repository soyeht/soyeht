//! A Task Scheduler task for the user, started at logon.
//!
//! - The trigger is a logon of this user only (`UserId`), not of anyone.
//! - The task runs as that user with the token of the interactive session
//!   (`InteractiveToken`) and no elevation (`LeastPrivilege`): the engine starts the person's
//!   harnesses, which need their profile and their session.
//! - It has no time limit, starts only one instance, and runs on battery.
//! - `RestartOnFailure` starts it again a minute after it ended with an error, up to the most
//!   the scheduler takes. Task Scheduler has no restart for a program that ended cleanly, and
//!   the engine and supervisor never end cleanly unless they were asked to.
//!
//! A task cannot set environment variables or redirect output, so a definition for it carries
//! neither (the program gets what it needs as arguments), and its output goes to the console
//! the scheduler gives it.

use super::xml_escape;
use crate::{definition::Definition, policy};

/// `text` as one argument of a Windows command line, quoted by the rules the C runtime and
/// `CommandLineToArgvW` read: a backslash is literal unless it precedes a double quote.
fn command_line_word(text: &str) -> String {
    if !text.is_empty() && !text.contains([' ', '\t', '"']) {
        return text.to_owned();
    }
    let mut word = String::from("\"");
    let mut backslashes = 0;
    for character in text.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                word.push_str(&"\\".repeat(backslashes * 2 + 1));
                word.push('"');
                backslashes = 0;
            }
            other => {
                word.push_str(&"\\".repeat(backslashes));
                word.push(other);
                backslashes = 0;
            }
        }
    }
    word.push_str(&"\\".repeat(backslashes * 2));
    word.push('"');
    word
}

pub(super) fn render(definition: &Definition) -> String {
    let user = definition
        .user
        .as_deref()
        .map(xml_escape)
        .map(|user| format!("      <UserId>{user}</UserId>\n"))
        .unwrap_or_default();
    let arguments: Vec<String> = definition
        .arguments
        .iter()
        .map(|argument| command_line_word(argument))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>{description}</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
{user}    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
{user}      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>{priority}</Priority>
    <RestartOnFailure>
      <Interval>{interval}</Interval>
      <Count>{count}</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        description = xml_escape(&definition.description),
        priority = policy::WINDOWS_PRIORITY,
        interval = policy::WINDOWS_RESTART_INTERVAL,
        count = policy::WINDOWS_RESTART_COUNT,
        command = xml_escape(&definition.program),
        arguments = xml_escape(&arguments.join(" ")),
    )
}

/// The bytes `schtasks /Create /XML` reads: UTF-16 little-endian with a byte order mark, the
/// encoding the XML declares and the one Windows itself writes when it exports a task.
pub fn encode_task_file(xml: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::sample;

    fn windows_sample() -> Definition {
        Definition {
            program: "C:\\Program Files\\Soyeht Next\\soyeht-engine.exe".into(),
            arguments: vec![
                "--profile".into(),
                "prod".into(),
                "--data-dir".into(),
                "C:\\Users\\A B\\AppData\\Local\\Soyeht.next".into(),
            ],
            environment: Default::default(),
            log: None,
            user: Some("S-1-5-21-1-2-3-1001".into()),
            ..sample("Soyeht.Next.Test.Engine")
        }
    }

    #[test]
    fn the_task_runs_at_this_users_logon_and_restarts_after_a_failure() {
        let text = render(&windows_sample());
        assert!(
            text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n<Task version=\"1.2\"")
        );
        assert!(text.contains(
            "<LogonTrigger>\n      <Enabled>true</Enabled>\n      <UserId>S-1-5-21-1-2-3-1001</UserId>\n    </LogonTrigger>"
        ));
        assert!(text.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(text.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(text.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(text.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(text.contains(
            "<RestartOnFailure>\n      <Interval>PT1M</Interval>\n      <Count>999</Count>\n    </RestartOnFailure>"
        ));
        assert!(
            text.contains("<Command>C:\\Program Files\\Soyeht Next\\soyeht-engine.exe</Command>")
        );
        assert!(text.contains(
            "<Arguments>--profile prod --data-dir \"C:\\Users\\A B\\AppData\\Local\\Soyeht.next\"</Arguments>"
        ));
    }

    #[test]
    fn a_task_for_nobody_in_particular_names_no_user() {
        let mut definition = windows_sample();
        definition.user = None;
        assert!(!render(&definition).contains("<UserId>"));
    }

    #[test]
    fn arguments_are_quoted_the_way_windows_reads_them() {
        assert_eq!(command_line_word("plain"), "plain");
        assert_eq!(command_line_word(""), "\"\"");
        assert_eq!(command_line_word("a b"), "\"a b\"");
        assert_eq!(command_line_word("a\\b"), "a\\b");
        assert_eq!(command_line_word("say \"hi\""), "\"say \\\"hi\\\"\"");
        // Backslashes before the closing quote, and before a quote, are doubled.
        assert_eq!(command_line_word("C:\\My Dir\\"), "\"C:\\My Dir\\\\\"");
        assert_eq!(command_line_word("a\\\"b"), "\"a\\\\\\\"b\"");
    }

    #[test]
    fn the_file_is_utf16_little_endian_with_a_byte_order_mark() {
        assert_eq!(
            encode_task_file("A\u{e9}\u{1f600}"),
            [0xFF, 0xFE, 0x41, 0x00, 0xE9, 0x00, 0x3D, 0xD8, 0x00, 0xDE]
        );
    }
}

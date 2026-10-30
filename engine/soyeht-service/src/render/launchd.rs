//! A launchd user agent (`~/Library/LaunchAgents/<label>.plist`).
//!
//! `KeepAlive` restarts the program however it ended, but only while the program is there
//! (`PathState`): an app dragged to the Trash must not leave an agent that fails to start every
//! second for ever. `RunAtLoad` starts it when the agent loads, which is at login. `ProcessType` `Interactive` keeps the system from throttling a
//! program a person is waiting on.

use super::xml_escape;
use crate::{definition::Definition, policy};

const HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
"#;

pub(super) fn render(definition: &Definition) -> String {
    let mut text = String::from(HEADER);
    text.push_str(&format!(
        "  <key>Label</key>\n  <string>{}</string>\n  <key>ProgramArguments</key>\n  <array>\n",
        xml_escape(&definition.name)
    ));
    for argument in std::iter::once(&definition.program).chain(&definition.arguments) {
        text.push_str(&format!("    <string>{}</string>\n", xml_escape(argument)));
    }
    text.push_str("  </array>\n");
    if !definition.environment.is_empty() {
        text.push_str("  <key>EnvironmentVariables</key>\n  <dict>\n");
        for (name, value) in &definition.environment {
            text.push_str(&format!(
                "    <key>{}</key>\n    <string>{}</string>\n",
                xml_escape(name),
                xml_escape(value)
            ));
        }
        text.push_str("  </dict>\n");
    }
    text.push_str(&format!(
        r#"  <key>KeepAlive</key>
  <dict>
    <key>PathState</key>
    <dict>
      <key>{}</key>
      <true/>
    </dict>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>ThrottleInterval</key>
  <integer>{}</integer>
  <key>ProcessType</key>
  <string>{}</string>
"#,
        xml_escape(&definition.program),
        policy::RESTART_DELAY_SECONDS,
        policy::LAUNCHD_PROCESS_TYPE
    ));
    if let Some(log) = &definition.log {
        let log = xml_escape(log);
        text.push_str(&format!(
            r#"  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
"#
        ));
    }
    text.push_str("</dict>\n</plist>\n");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::sample;

    #[test]
    fn the_agent_runs_at_login_and_restarts_however_it_ended_while_its_program_exists() {
        let text = render(&sample("com.soyeht.next.test.engine"));
        assert_eq!(
            text,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.soyeht.next.test.engine</string>
  <key>ProgramArguments</key>
  <array>
    <string>/opt/next/soyeht-engine</string>
    <string>--data-dir</string>
    <string>/data/next</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>/usr/bin</string>
  </dict>
  <key>KeepAlive</key>
  <dict>
    <key>PathState</key>
    <dict>
      <key>/opt/next/soyeht-engine</key>
      <true/>
    </dict>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>ThrottleInterval</key>
  <integer>1</integer>
  <key>ProcessType</key>
  <string>Interactive</string>
  <key>StandardOutPath</key>
  <string>/data/next/logs/engine.log</string>
  <key>StandardErrorPath</key>
  <string>/data/next/logs/engine.log</string>
</dict>
</plist>
"#
        );
    }

    #[test]
    fn a_path_with_markup_stays_one_string() {
        let mut definition = sample("com.soyeht.next.test.engine");
        definition.program = "/Apps/A & B <next>/soyeht-engine".into();
        definition.environment.clear();
        definition.log = None;
        let text = render(&definition);
        assert!(text.contains("<string>/Apps/A &amp; B &lt;next&gt;/soyeht-engine</string>"));
        // The path that keeps the agent alive is the same program, escaped the same way.
        assert!(text.contains("<key>/Apps/A &amp; B &lt;next&gt;/soyeht-engine</key>"));
        assert!(!text.contains("EnvironmentVariables"));
        assert!(!text.contains("StandardOutPath"));
    }
}

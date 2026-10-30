//! The text of each system's service file, from a [`Definition`]. Pure: nothing here touches a
//! file or runs a command, so the three renderings are tested on every system.

mod launchd;
mod systemd;
mod task;

use crate::{definition::Definition, error::Result, system::System};

pub use task::encode_task_file;

/// Renders `definition` as the file `system` reads: a launchd property list, a systemd user
/// unit or a Task Scheduler task (XML). Refuses a definition that cannot be written faithfully.
pub fn render(system: System, definition: &Definition) -> Result<String> {
    definition.check()?;
    Ok(match system {
        System::Macos => launchd::render(definition),
        System::Linux => systemd::render(definition),
        System::Windows => task::render(definition),
    })
}

/// `text` as it may appear inside an XML element. The files hold no user text in an attribute,
/// so quotes need no escape.
pub(crate) fn xml_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// The inverse of [`xml_escape`]: the text of an element read back from a file we wrote.
pub(crate) fn xml_unescape(text: &str) -> String {
    // `&amp;` last, so that `&amp;lt;` reads back as `&lt;` and not as `<`.
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::sample;

    #[test]
    fn xml_text_cannot_close_an_element_or_start_another() {
        assert_eq!(
            xml_unescape(&xml_escape("a & b <c> &lt; &amp;")),
            "a & b <c> &lt; &amp;"
        );
        assert_eq!(
            xml_escape(r#"<a href="x">&</a>"#),
            r#"&lt;a href="x"&gt;&amp;&lt;/a&gt;"#
        );
    }

    #[test]
    fn every_system_refuses_a_definition_that_could_name_the_legacy_app() {
        for system in [System::Macos, System::Linux, System::Windows] {
            assert!(render(system, &sample(&crate::guard::legacy_values::name())).is_err());
            assert!(render(system, &sample("com.soyeht.next.test.engine")).is_ok());
        }
    }
}

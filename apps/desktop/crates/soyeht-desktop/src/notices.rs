//! The standing notes about this client, such as drawing on a software renderer. A note is not a
//! failure of a request: it stays across connections, and the person dismisses it for good. The
//! dismissal is one of the device's own choices (`LocalPreferences`), kept beside the theme.

use crate::workbench::Workbench;
use gpui::Context;
use soyeht_client::local_preferences::LocalPreferences;
use std::path::Path;

/// The note that the window draws on a software renderer.
pub(crate) const SOFTWARE_RENDERER: &str = "softwareRenderer";

/// Keeps the note `id` dismissed, with the other choices in the file.
fn remember_dismissed(directory: &Path, id: &str) -> std::io::Result<()> {
    let mut saved = LocalPreferences::load(directory);
    saved.dismiss(id);
    saved.save(directory)
}

/// Whether the person dismissed the note `id` for good; `directory` is where the device keeps its
/// choices, `None` when it keeps none.
fn is_dismissed(directory: Option<&Path>, id: &str) -> bool {
    directory.is_some_and(|directory| LocalPreferences::load(directory).is_dismissed(id))
}

impl Workbench {
    /// Shows the note `id` under the top bar, unless the person dismissed it for good.
    pub(crate) fn show_notice(&mut self, id: &'static str, text: String) {
        if !is_dismissed(self.preferences.as_deref(), id) {
            self.state.notice = Some(text);
            self.notice_id = Some(id);
        }
    }

    /// The person closed the note: it does not come back on the next launch.
    pub(crate) fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.state.notice = None;
        if let (Some(id), Some(directory)) = (self.notice_id.take(), self.preferences.as_deref())
            && let Err(error) = remember_dismissed(directory, id)
        {
            soyeht_platform::log::failure("desktop.notice_dismissal_not_saved", &error);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_shown_until_it_is_dismissed_and_then_never_again() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-notices-a-{}", std::process::id()));
        assert!(!is_dismissed(Some(&directory), SOFTWARE_RENDERER));
        remember_dismissed(&directory, SOFTWARE_RENDERER).unwrap();
        assert!(is_dismissed(Some(&directory), SOFTWARE_RENDERER));
        assert!(!is_dismissed(Some(&directory), "another"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn dismissing_a_note_keeps_the_chosen_theme() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-notices-b-{}", std::process::id()));
        let chosen = LocalPreferences {
            theme: Some("neoDeepForest".into()),
            ..LocalPreferences::default()
        };
        chosen.save(&directory).unwrap();
        remember_dismissed(&directory, SOFTWARE_RENDERER).unwrap();
        assert_eq!(
            LocalPreferences::load(&directory).theme.as_deref(),
            Some("neoDeepForest")
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_device_that_keeps_no_choices_dismisses_nothing_for_good() {
        assert!(!is_dismissed(None, SOFTWARE_RENDERER));
    }
}

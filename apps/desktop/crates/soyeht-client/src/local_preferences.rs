//! What this device keeps for itself, apart from the engine's settings: the choices that differ
//! from one machine to the next, as the Mac keeps its theme in its own user defaults. They live
//! in one small file beside the engine's socket, in the profile directory.

use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The file, inside the profile directory.
const FILE: &str = "client-preferences.json";

/// The device's own choices. A name the file does not carry is not chosen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPreferences {
    /// The theme the person picked, by the name the Mac persists (`neoDeepForest`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// The standing notes the person dismissed for good, by id (`softwareRenderer`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dismissed_notices: Vec<String>,
}

/// The directory the file lives in: the one `socket` is in, so a disposable engine
/// (`SOYEHT_ENGINE_SOCKET`) keeps its own choices too.
pub fn directory(socket: &Path) -> PathBuf {
    socket.parent().map(Path::to_path_buf).unwrap_or_default()
}

impl LocalPreferences {
    /// Whether the person dismissed the note `id` for good.
    pub fn is_dismissed(&self, id: &str) -> bool {
        self.dismissed_notices
            .iter()
            .any(|dismissed| dismissed == id)
    }

    /// Keeps the note `id` dismissed.
    pub fn dismiss(&mut self, id: &str) {
        if !self.is_dismissed(id) {
            self.dismissed_notices.push(id.to_owned());
        }
    }

    /// What `directory` holds. A missing file is a first run; a file that cannot be read or
    /// parsed counts as empty and is logged, so a damaged file never stops the window opening.
    pub fn load(directory: &Path) -> Self {
        match fs::read_to_string(directory.join(FILE)) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| {
                soyeht_platform::log::event("desktop.local_preferences_invalid", &[]);
                Self::default()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                soyeht_platform::log::failure("desktop.local_preferences_unreadable", &error);
                Self::default()
            }
        }
    }

    /// Writes the choices to `directory`, replacing the file in one step so a crash leaves
    /// either the old file or the new one.
    pub fn save(&self, directory: &Path) -> io::Result<()> {
        fs::create_dir_all(directory)?;
        // An owner-only write needs an owner-only directory (Windows files take its access list).
        soyeht_platform::fs::set_private_dir(directory)?;
        soyeht_platform::fs::write_atomic(
            &directory.join(FILE),
            &serde_json::to_vec_pretty(self)?,
            soyeht_platform::fs::WriteMode::OwnerOnly,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("soyeht-local-prefs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn a_saved_choice_comes_back_and_the_directory_is_made_on_the_way() {
        let directory = scratch("roundtrip").join("nested");
        let chosen = LocalPreferences {
            theme: Some("neoDeepForest".into()),
            ..LocalPreferences::default()
        };
        chosen.save(&directory).unwrap();
        assert_eq!(LocalPreferences::load(&directory), chosen);
        let replaced = LocalPreferences::default();
        replaced.save(&directory).unwrap();
        assert_eq!(LocalPreferences::load(&directory), replaced);
        let _ = fs::remove_dir_all(directory.parent().unwrap());
    }

    #[test]
    fn a_dismissed_note_stays_dismissed_and_keeps_the_theme() {
        let directory = scratch("dismissed");
        let mut saved = LocalPreferences {
            theme: Some("neoDeepForest".into()),
            ..LocalPreferences::default()
        };
        assert!(!saved.is_dismissed("softwareRenderer"));
        saved.dismiss("softwareRenderer");
        saved.dismiss("softwareRenderer");
        saved.save(&directory).unwrap();
        let loaded = LocalPreferences::load(&directory);
        assert!(loaded.is_dismissed("softwareRenderer"));
        assert!(!loaded.is_dismissed("other"));
        assert_eq!(loaded.dismissed_notices.len(), 1);
        assert_eq!(loaded.theme.as_deref(), Some("neoDeepForest"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_file_from_before_notes_could_be_dismissed_still_loads() {
        let directory = scratch("older");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(FILE), r#"{"theme":"neoMilk"}"#).unwrap();
        let loaded = LocalPreferences::load(&directory);
        assert_eq!(loaded.theme.as_deref(), Some("neoMilk"));
        assert!(loaded.dismissed_notices.is_empty());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_choices_are_kept_beside_the_socket() {
        assert_eq!(
            directory(Path::new("/run/soyeht/engine.sock")),
            PathBuf::from("/run/soyeht")
        );
    }

    #[test]
    fn a_first_run_and_a_damaged_file_both_start_empty() {
        let directory = scratch("empty");
        assert_eq!(
            LocalPreferences::load(&directory),
            LocalPreferences::default()
        );
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(FILE), "{ not json").unwrap();
        assert_eq!(
            LocalPreferences::load(&directory),
            LocalPreferences::default()
        );
        let _ = fs::remove_dir_all(&directory);
    }
}

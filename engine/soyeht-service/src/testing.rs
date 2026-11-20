//! What the tests of this crate share.

use std::{fs, path::PathBuf};

/// `path`, when it is written as an absolute Unix path, as an absolute path of this system:
/// Windows needs a drive before a path is absolute, and writes its separators another way. Any
/// other text is the path as it is.
pub(crate) fn absolute(path: &str) -> PathBuf {
    if cfg!(windows) && path.starts_with('/') {
        PathBuf::from(format!("C:{}", path.replace('/', "\\")))
    } else {
        PathBuf::from(path)
    }
}

/// A folder of its own for one test, removed when the test ends. Its name carries "next" like
/// every place of the app.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn new(label: &str) -> Self {
        Self::under(std::env::temp_dir(), label)
    }

    /// A folder short enough for the path of a Unix socket, which the long temporary folder of
    /// macOS is not.
    pub(crate) fn short(label: &str) -> Self {
        Self::under(
            if cfg!(unix) {
                PathBuf::from("/tmp")
            } else {
                std::env::temp_dir()
            },
            label,
        )
    }

    fn under(parent: PathBuf, label: &str) -> Self {
        let root = parent.join(format!("next-service-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

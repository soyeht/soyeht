//! `file:` URIs: the form in which a language server names files, and the paths they stand for.
//!
//! On Unix `file:///tmp/a%20b.rs` is the path `/tmp/a b.rs`. On Windows `file:///C:/a%20b.rs` is
//! `C:\a b.rs`: the drive letter follows the slash and the separators are forward slashes.

use std::path::{Path, PathBuf};

/// The URI of an absolute `path`, or `None` when the path is not UTF-8 or not absolute.
pub(super) fn path_to_uri(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }
    let text = path.to_str()?;
    #[cfg(windows)]
    {
        let text = text
            .strip_prefix(r"\\?\")
            .unwrap_or(text)
            .replace('\\', "/");
        let (drive, rest) = drive_split(&text)?;
        Some(format!(
            "file:///{drive}:{}",
            percent_encode(rest.as_bytes())
        ))
    }
    #[cfg(not(windows))]
    {
        Some(format!("file://{}", percent_encode(text.as_bytes())))
    }
}

/// The path a `file:` URI names, or `None` when the URI is not a plain local file URI.
pub(super) fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    if !encoded.starts_with('/') || encoded.contains(['?', '#']) {
        return None;
    }
    let bytes = percent_decode(encoded.as_bytes())?;
    let Ok(text) = String::from_utf8(bytes) else {
        return None;
    };
    if text.contains('\0') {
        return None;
    }
    native_path(&text)
}

#[cfg(not(windows))]
fn native_path(text: &str) -> Option<PathBuf> {
    Some(PathBuf::from(text))
}

#[cfg(windows)]
fn native_path(text: &str) -> Option<PathBuf> {
    let (drive, rest) = drive_split(text.strip_prefix('/')?)?;
    Some(PathBuf::from(format!(
        "{drive}:{}",
        rest.replace('/', "\\")
    )))
}

/// Splits `C:/dir/file` into the drive letter and `/dir/file`.
#[cfg(windows)]
fn drive_split(text: &str) -> Option<(char, &str)> {
    let mut characters = text.chars();
    let drive = characters.next().filter(char::is_ascii_alphabetic)?;
    let rest = characters.as_str().strip_prefix(':')?;
    rest.starts_with('/').then_some((drive, rest))
}

fn percent_encode(bytes: &[u8]) -> String {
    let mut result = String::new();
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            result.push(char::from(*byte));
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

fn percent_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] != b'%' {
            output.push(input[index]);
            index += 1;
            continue;
        }
        let high = *input.get(index + 1)?;
        let low = *input.get(index + 2)?;
        let (high, low) = (
            char::from(high).to_digit(16)?,
            char::from(low).to_digit(16)?,
        );
        // Both digits are below 16, so the byte cannot overflow.
        output.push((high << 4 | low) as u8);
        index += 3;
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn file_uri_decoder_is_strict_and_utf8_aware() {
        assert_eq!(
            uri_to_path("file:///tmp/a%20b/%C3%A9.rs"),
            Some(PathBuf::from("/tmp/a b/é.rs"))
        );
        for invalid in [
            "https:///tmp/main.rs",
            "file://host/tmp/main.rs",
            "file:///tmp/%ZZ.rs",
            "file:///tmp/%00.rs",
            "file:///tmp/a.rs?query",
        ] {
            assert_eq!(uri_to_path(invalid), None);
        }
    }

    #[cfg(windows)]
    #[test]
    fn file_uri_decoder_is_strict_and_utf8_aware() {
        assert_eq!(
            uri_to_path("file:///C:/tmp/a%20b/%C3%A9.rs"),
            Some(PathBuf::from(r"C:\tmp\a b\é.rs"))
        );
        for invalid in [
            "https:///C:/tmp/main.rs",
            "file://host/C:/tmp/main.rs",
            "file:///tmp/main.rs",
            "file:///C:/tmp/%ZZ.rs",
            "file:///C:/tmp/%00.rs",
            "file:///C:/tmp/a.rs?query",
        ] {
            assert_eq!(uri_to_path(invalid), None);
        }
    }

    #[test]
    fn a_relative_path_has_no_uri() {
        assert_eq!(path_to_uri(Path::new("src/main.rs")), None);
    }

    #[test]
    fn a_path_survives_the_round_trip_through_a_uri() {
        let path = if cfg!(windows) {
            PathBuf::from(r"C:\work space\é\main.rs")
        } else {
            PathBuf::from("/work space/é/main.rs")
        };
        let uri = path_to_uri(&path).unwrap();
        assert!(uri.starts_with("file:///") || uri.starts_with("file://"));
        assert!(!uri.contains(' ') && !uri.contains('\\'));
        assert_eq!(uri_to_path(&uri), Some(path));
    }
}

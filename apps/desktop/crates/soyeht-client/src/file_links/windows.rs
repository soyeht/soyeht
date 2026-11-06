//! What the windows dialect adds to the posix pattern of [`super`]: `\` between components, a
//! drive or a UNC share as the root of an absolute path, a quoted path that holds whitespace, and
//! the MSVC `file(line,column)` location. The Mac's `FilePathLinks` reads the windows dialect the
//! same way, and the cases are in `contracts/vectors/file-paths.json`.

use super::{Dialect, Match, Place, colon_place, digits, in_path, path_shape, reference_of};
use soyeht_protocol::limits::EDITOR_PATH_BYTES;

/// A path with its root, if it has one, and the components after it joined with `/`.
pub(super) struct Split {
    /// `C:/` or `//server/share/` as written (with `/` for `\`), or empty for a relative path.
    pub root: String,
    pub rest: Vec<char>,
}

pub(super) fn split_root(dialect: Dialect, path: &[char]) -> Split {
    if !dialect.windows() {
        return Split {
            root: String::new(),
            rest: path.to_vec(),
        };
    }
    let normal: Vec<char> = path
        .iter()
        .map(|&character| if character == '\\' { '/' } else { character })
        .collect();
    if drive_length(dialect, path) > 0 {
        return Split {
            root: normal[..3].iter().collect(),
            rest: normal[3..].to_vec(),
        };
    }
    if path.starts_with(&['\\', '\\']) {
        let mut parts = normal[2..].splitn(3, |&character| character == '/');
        if let (Some(server), Some(share), Some(rest)) = (parts.next(), parts.next(), parts.next())
            && named(server)
            && named(share)
        {
            let name = |part: &[char]| part.iter().collect::<String>();
            return Split {
                root: format!("//{}/{}/", name(server), name(share)),
                rest: rest.to_vec(),
            };
        }
    }
    Split {
        root: String::new(),
        rest: normal,
    }
}

fn named(part: &[char]) -> bool {
    !part.is_empty() && part != ['.'] && part != ['.', '.']
}

/// The path as one string with `/` between components, to compare as text.
fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

/// Whether resolving `path` against the workspace `root` would reach beyond this computer's own
/// drives, or a device: a share (`\\server\share`, which Windows opens by contacting the server
/// and authenticating to it), the WebDAV form of one (`\\server@SSL\share`), a verbatim share
/// (`\\?\UNC\`) or a device (`\\.\`, `\\?\`): every path that starts with two separators.
/// Only text is compared, so nothing is asked of the system.
///
/// The one exception is a path under a workspace that is itself on a share, by the text of its
/// root: the person chose that share, and a path outside it is refused as before.
pub(super) fn leaves_the_machine(dialect: Dialect, root: &str, path: &str) -> bool {
    if !dialect.windows() {
        return false;
    }
    let path = slashed(path);
    if !path.starts_with("//") {
        return false;
    }
    let root = slashed(root);
    let root = root.trim_end_matches('/');
    let share = root.starts_with("//") && !root.starts_with("//?/") && !root.starts_with("//./");
    let inside = path
        .to_lowercase()
        .strip_prefix(&root.to_lowercase())
        .is_some_and(|rest| rest.starts_with('/'));
    !(share && inside)
}

/// How many characters a drive takes at the start of `text` (`C:` of `C:\x`), or 0 when `text`
/// does not start with a drive and a separator.
pub(super) fn drive_length(dialect: Dialect, text: &[char]) -> usize {
    let drive = dialect.windows()
        && matches!(text, [letter, ':', '\\' | '/', ..] if letter.is_ascii_alphabetic());
    if drive { 2 } else { 0 }
}

pub(super) fn is_quote(character: char) -> bool {
    character == '"' || character == '\''
}

/// `(line)` or `(line,column)` at `open`, as the MSVC compilers print a location.
pub(super) fn parenthesised_place(characters: &[char], open: usize) -> Option<Place> {
    let (line, line_end) = digits(characters, open + 1)?;
    let (column, end) = if characters.get(line_end) == Some(&',') {
        let (value, column_end) = digits(characters, line_end + 1)?;
        (Some(value), column_end)
    } else {
        (None, line_end)
    };
    (characters.get(end) == Some(&')')).then_some(Place {
        line,
        column,
        end: end + 1,
    })
}

/// A path in quotes that holds a space or another character a path is not made of, with its
/// location inside the quotes: `"C:\my dir\a.rs:4"`. The quotes belong to the label. A path of
/// plain characters is found without its quotes, and a quote with no closing one is only a quote.
pub(super) fn quoted(characters: &[char], start: usize) -> Option<Match> {
    let quote = characters[start];
    let window = &characters[start + 1..characters.len().min(start + 1 + EDITOR_PATH_BYTES + 32)];
    let length = window
        .iter()
        .position(|&character| character == quote || character.is_control())?;
    if window[length] != quote {
        return None;
    }
    let inner = &window[..length];
    // The last `:line` or `:line:column` that runs to the closing quote.
    let (colon, place) = (0..inner.len())
        .filter(|&at| inner[at] == ':')
        .find_map(|at| {
            let place = colon_place(inner, at)?;
            (place.end == inner.len()).then_some((at, place))
        })?;
    let path = &inner[..colon];
    let sound = path.iter().enumerate().all(|(at, &character)| {
        !character.is_control()
            && !"\"'<>|*?".contains(character)
            && (character != ':' || at == 1 && drive_length(Dialect::Windows, path) > 0)
    });
    let beyond_a_plain_path = path
        .iter()
        .any(|&character| character != ':' && !in_path(Dialect::Windows, character));
    (sound && beyond_a_plain_path && path_shape(Dialect::Windows, path)).then(|| Match {
        start,
        end: start + length + 2,
        reference: reference_of(Dialect::Windows, path, place),
    })
}

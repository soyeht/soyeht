//! File references in text: a path with a line, as `src/main.rs:12` or `./docs/guide.md:3:9`,
//! in an agent's reply or in terminal output.
//!
//! The Mac links the same text (`FilePathLinks`); the cases both clients pass are in
//! `contracts/vectors/file-paths.json`. A reference is a path of the workspace, with a directory
//! or a dot in its name, then `:line` and optionally `:column`, counted from 1 in the text and
//! from 0 in a [`Reference`]. The posix pattern is the Mac's, character for character: the path is
//! made of letters, digits and `_ @ + . - /`, the character before it is not one of
//! `A-Z a-z 0-9 _ . / -`, and the character after the numbers is not a letter, a digit, `_` or `:`.
//! A path that is absolute or has an empty, `.` or `..` component is text, not a reference.
//!
//! The text of a Windows workspace is read by the windows [`Dialect`] (the module `windows`
//! holds its rules): `\` separates like `/`, a drive (`C:\x.rs:3`) or a UNC share
//! (`\\server\share\x.rs:3`) makes a path absolute, a path may be quoted to hold example, and
//! the MSVC `file(line,column)` form is a location too. On a posix system a backslash is a legal
//! character of a file name that no tool prints in a path, so it is not one of the path's
//! characters there and the text after it is read as before. Case is never folded here: the path
//! is kept as written and [`resolve`] lets the file system decide, which on Windows gives the
//! name's real case.
//!
//! Whether the file exists in the workspace is [`resolve`]'s question: a reference to a file
//! that is not there, or whose real place is outside the workspace, is not a link.
//! The Mac does no existence check (`FilePathLinks` never asks the disk), so [`resolve`] is a rule
//! only this client has: it is not in the vector, and its cases are in `file_links/tests.rs`. It
//! does blocking IO, so the window calls it through `Files::check` in `soyeht-desktop`, which
//! runs it off the UI thread, and never directly.

use soyeht_platform::fs::canonicalize;
use soyeht_protocol::{client_policy::CAPS_FILE_LINKS, limits::EDITOR_PATH_BYTES};
use std::{
    ops::Range,
    path::{Component, Path},
};

mod windows;

/// How the paths of a workspace are written: the system the workspace is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Posix,
    Windows,
}

impl Dialect {
    /// The dialect of the system this client runs on. The desktop client talks to the engine of
    /// its own machine, so its workspaces are written the way the system writes paths.
    pub const HOST: Dialect = if cfg!(windows) {
        Dialect::Windows
    } else {
        Dialect::Posix
    };

    fn windows(self) -> bool {
        self == Dialect::Windows
    }
}

/// Where a reference points, counted from 0 as the engine counts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reference {
    /// With `/` between components and no leading `./`. Relative to the workspace, or, in the
    /// windows dialect, absolute with its drive (`C:/x/y.rs`) or share (`//server/share/y.rs`)
    /// as written, which [`resolve`] accepts only inside the workspace.
    pub path: String,
    pub line: u32,
    pub column: Option<u32>,
}

/// A reference in a text, and the characters it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub range: Range<usize>,
    pub reference: Reference,
}

/// A reference that points at a file of the workspace: what to ask the engine to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    /// The file's path from the workspace's folder, with `/` between components.
    pub path: String,
    pub line: u32,
    pub column: Option<u32>,
}

/// The references in `text`, in order, with the byte range of each.
pub fn find(dialect: Dialect, text: &str) -> Vec<Found> {
    let characters: Vec<char> = text.chars().collect();
    let mut offsets: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    offsets.push(text.len());
    find_chars(dialect, &characters)
        .into_iter()
        .map(|found| Found {
            range: offsets[found.range.start]..offsets[found.range.end],
            reference: found.reference,
        })
        .collect()
}

/// The references in `characters`, with the range of characters each takes.
pub fn find_chars(dialect: Dialect, characters: &[char]) -> Vec<Found> {
    let mut found = Vec::new();
    let mut from = 0;
    while found.len() < CAPS_FILE_LINKS {
        let Some(matched) = next_match(dialect, characters, from) else {
            break;
        };
        from = matched.end;
        // A label that matches the pattern but is not a reference (`../x.rs:1`) is plain text,
        // and takes its characters with it.
        if let Some(reference) = matched.reference {
            found.push(Found {
                range: matched.start..matched.end,
                reference,
            });
        }
    }
    found
}

/// The reference that takes the character at `index`, when one does.
pub fn find_at(dialect: Dialect, characters: &[char], index: usize) -> Option<Found> {
    let mut from = 0;
    while let Some(matched) = next_match(dialect, characters, from) {
        if matched.start > index {
            return None;
        }
        from = matched.end;
        if index < matched.end {
            return matched.reference.map(|reference| Found {
                range: matched.start..matched.end,
                reference,
            });
        }
    }
    None
}

/// What opening `reference` in the workspace rooted at `root` comes to, or `None` when the file
/// is not there or is really somewhere outside the workspace (a symbolic link out of it, or an
/// absolute path of another folder). The file system answers for the case of names, and the path
/// that opens is the canonical one, so it has the name's real case.
pub fn resolve(root: &Path, reference: &Reference) -> Option<Opening> {
    // A file that cannot be reached is not a link, whatever the reason.
    let (Ok(file), Ok(base)) = (
        canonicalize(&root.join(&reference.path)),
        canonicalize(root),
    ) else {
        return None;
    };
    if !file.is_file() {
        return None;
    }
    let Ok(inside) = file.strip_prefix(&base) else {
        return None;
    };
    let parts: Vec<&str> = inside
        .components()
        .map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<_>>()?;
    (!parts.is_empty()).then(|| Opening {
        path: parts.join("/"),
        line: reference.line,
        column: reference.column,
    })
}

/// A place in the text where the pattern matches, whether or not it is a reference.
struct Match {
    start: usize,
    end: usize,
    reference: Option<Reference>,
}

/// The numbers after a path, and the characters up to where they end. A number is `None` when it
/// does not fit a `u32`, which makes the label text and not a reference.
struct Place {
    line: Option<u32>,
    column: Option<Option<u32>>,
    end: usize,
}

fn next_match(dialect: Dialect, characters: &[char], from: usize) -> Option<Match> {
    // The end of the run of path characters that holds the start being tried.
    let mut run_end = 0;
    for start in from..characters.len() {
        if start > 0 && before_path(dialect, characters[start - 1]) {
            continue;
        }
        if dialect.windows() && windows::is_quote(characters[start]) {
            if let Some(matched) = windows::quoted(characters, start) {
                return Some(matched);
            }
            continue;
        }
        if start >= run_end {
            let body = start + windows::drive_length(dialect, &characters[start..]);
            run_end = characters[body..]
                .iter()
                .position(|&character| !in_path(dialect, character))
                .map_or(characters.len(), |length| body + length);
        }
        if let Some(matched) = match_at(dialect, characters, start, run_end) {
            return Some(matched);
        }
    }
    None
}

/// Whether the pattern matches from `start`, where the run of path characters ends at `run_end`.
fn match_at(dialect: Dialect, characters: &[char], start: usize, run_end: usize) -> Option<Match> {
    // A run this long is no path (it would be refused below); skipping it keeps the search linear.
    if run_end - start > EDITOR_PATH_BYTES + 2 {
        return None;
    }
    let place = place_after(dialect, characters, run_end)?;
    let path = &characters[start..run_end];
    if !path_shape(dialect, path) {
        return None;
    }
    Some(Match {
        start,
        end: place.end,
        reference: reference_of(dialect, path, place),
    })
}

/// The numbers that follow the path, which ends at `at`: `:line` and optionally `:column`, or, in
/// the windows dialect, the MSVC `(line)` and `(line,column)`.
fn place_after(dialect: Dialect, characters: &[char], at: usize) -> Option<Place> {
    match characters.get(at)? {
        ':' => colon_place(characters, at),
        '(' if dialect.windows() => windows::parenthesised_place(characters, at),
        _ => None,
    }
}

/// `:line` or `:line:column` at `colon`. The `:` after the line must start a column, which then
/// must not be followed by a name character or another `:`.
fn colon_place(characters: &[char], colon: usize) -> Option<Place> {
    let (line, line_end) = digits(characters, colon + 1)?;
    let mut end = line_end;
    let mut column = None;
    if characters.get(line_end) == Some(&':') {
        let (value, column_end) = digits(characters, line_end + 1)?;
        end = column_end;
        column = Some(value);
    }
    if characters.get(end).is_some_and(|&next| after_numbers(next)) {
        return None;
    }
    Some(Place { line, column, end })
}

/// The reference a matching label stands for, or `None` for text that only looks like one.
fn reference_of(dialect: Dialect, path: &[char], place: Place) -> Option<Reference> {
    let split = windows::split_root(dialect, path);
    let rest: String = split.rest.iter().collect();
    // A leading `./` is dropped from a path of the workspace, not from one that has a root.
    let rest = match split.root.is_empty() {
        true => rest.strip_prefix("./").unwrap_or(&rest),
        false => &rest,
    };
    let usable = !rest.is_empty()
        && split.root.len() + rest.len() <= EDITOR_PATH_BYTES
        && !rest.starts_with('/')
        && rest
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if !usable {
        return None;
    }
    let line = place.line?;
    let column = match place.column {
        None => None,
        Some(value) => Some(value?.checked_sub(1)?),
    };
    Some(Reference {
        path: format!("{}{rest}", split.root),
        line: line.checked_sub(1)?,
        column,
    })
}

/// A number that starts at `at` with a digit but 0, and the index after it. The number is `None`
/// when it does not fit a `u32`, which makes the label text and not a reference.
fn digits(characters: &[char], at: usize) -> Option<(Option<u32>, usize)> {
    if !characters
        .get(at)
        .is_some_and(|&first| ('1'..='9').contains(&first))
    {
        return None;
    }
    let length = characters[at..]
        .iter()
        .take_while(|character| character.is_ascii_digit())
        .count();
    let end = at + length;
    // A number that does not fit makes the label text, not a reference.
    let number = characters[at..end].iter().try_fold(0u32, |sum, digit| {
        sum.checked_mul(10)?.checked_add(digit.to_digit(10)?)
    });
    Some((number, end))
}

/// Whether the characters, up to the `:`, have the shape of a path: a directory and a name, or a
/// name with a dot inside it, after an optional `./`. A root (a drive, a share) is not counted.
fn path_shape(dialect: Dialect, path: &[char]) -> bool {
    let inner = |name: &[char]| {
        if name.contains(&'/') {
            let mut parts = name.split(|&character| character == '/');
            let all_named = parts.clone().all(|part| !part.is_empty());
            all_named && parts.nth(1).is_some()
        } else {
            name.len() >= 3 && name[1..name.len() - 1].contains(&'.')
        }
    };
    let rest = windows::split_root(dialect, path).rest;
    inner(&rest) || rest.starts_with(&['.', '/']) && inner(&rest[2..])
}

fn in_path(dialect: Dialect, character: char) -> bool {
    character.is_ascii_alphanumeric()
        || "_@+.-/".contains(character)
        || dialect.windows() && character == '\\'
}

/// A character that, right before a path, makes it the tail of a longer word.
fn before_path(dialect: Dialect, character: char) -> bool {
    character.is_ascii_alphanumeric()
        || "_./-".contains(character)
        || dialect.windows() && character == '\\'
}

fn after_numbers(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == ':'
}

#[cfg(test)]
mod tests;

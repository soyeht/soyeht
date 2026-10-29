//! Taking out of a line of text what a person should not paste into a public issue: the home
//! folder, the user's name, the machine's name and addresses, every path, every link, and
//! anything that looks like a token or a secret.
//!
//! The rules read the text and nothing else (the home folder, user name and host name are given
//! to [`Redactor::new`]), so they are the same on every system and a test feeds them any line.
//! They err toward removing: a relative path of two folders or more, a long mixed run of letters
//! and digits, and the rest of a line after an absolute path are removed even when harmless.

mod network;
mod paths;
mod secrets;
#[cfg(test)]
mod tests;

use std::{borrow::Cow, path::Path};

const USER: &str = "<user>";
const HOST: &str = "<host>";

/// A user or host name shorter than this is not replaced as a word: it would also cut other
/// words ("al" in "alpha"). Such a name is still removed wherever a path or an address shows it.
const NAME_MIN: usize = 3;

/// What to remove from a line, besides what the text itself shows.
#[derive(Clone, Debug, Default)]
pub struct Redactor {
    home: Option<String>,
    user: Option<String>,
    /// The machine's name, and its first label (`studio` of `studio.local`).
    hosts: Vec<String>,
}

impl Redactor {
    /// A redactor for the person and the machine running this program.
    pub fn current() -> Self {
        let user = match std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
            Ok(name) if !name.is_empty() => Some(name),
            // Not set, or not text: the name is then only removed where a path shows it.
            _ => None,
        };
        Self::new(
            std::env::home_dir().as_deref(),
            user.as_deref(),
            crate::hostname::get().as_deref(),
        )
    }

    pub fn new(home: Option<&Path>, user: Option<&str>, host: Option<&str>) -> Self {
        let long_enough = |name: &str| name.chars().count() >= NAME_MIN;
        let mut hosts: Vec<String> = host.into_iter().map(str::to_owned).collect();
        if let Some(first) = host.and_then(|name| name.split('.').next()) {
            hosts.push(first.to_owned());
        }
        hosts.retain(|name| long_enough(name));
        hosts.dedup();
        // The full name first, so its first label is not cut out of it.
        hosts.sort_by_key(|name| std::cmp::Reverse(name.len()));
        Self {
            home: home
                .map(|home| {
                    home.to_string_lossy()
                        .trim_end_matches(['/', '\\'])
                        .to_owned()
                })
                .filter(|home| home.len() > 1),
            user: user.filter(|user| long_enough(user)).map(str::to_owned),
            hosts,
        }
    }

    /// `text` with the home folder, paths, links, secrets, addresses, the machine's name and the
    /// user's name taken out.
    pub fn redact(&self, text: &str) -> String {
        let text = self.without_home(text);
        let text = paths::without_paths(&text);
        let text = network::without_addresses(&text);
        let text = secrets::without_secret_fields(&text);
        let text = secrets::without_tokens(&text);
        let text = secrets::without_emails(&text);
        let text = self
            .hosts
            .iter()
            .fold(text, |text, host| without_name(&text, host, HOST));
        match &self.user {
            Some(user) => without_name(&text, user, USER),
            None => text,
        }
    }

    /// Every spelling of the home folder, so one that sits inside a longer word is also found.
    fn without_home(&self, text: &str) -> String {
        let Some(home) = &self.home else {
            return text.to_owned();
        };
        // A JSON line writes a Windows backslash twice.
        let escaped = home.replace('\\', "\\\\");
        let slashes = home.replace('\\', "/");
        let mut text = text.replace(&escaped, "~");
        text = text.replace(home.as_str(), "~");
        text.replace(&slashes, "~")
    }
}

/// `text` with `name` replaced by `label` wherever it stands as a word, in any case, with or
/// without a plural or possessive `s` after it ("Caios-Mac-Studio" for the user "caio"). A name
/// that only starts or ends inside a longer word ("alexander" for "alex") stays.
fn without_name(text: &str, name: &str, label: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let wanted = name.to_ascii_lowercase();
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut search = 0;
    while let Some(found) = lower[search..].find(&wanted) {
        let at = search + found;
        let mut end = at + wanted.len();
        search = end;
        if word(text[..at].chars().next_back()) {
            continue;
        }
        // The plural or possessive: an `s`, with or without an apostrophe.
        let tail = &text[end..];
        let plural = ["'s", "\u{2019}s", "s"].iter().find(|suffix| {
            tail.get(..suffix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(suffix))
        });
        if let Some(suffix) = plural
            && !word(tail[suffix.len()..].chars().next())
        {
            end += suffix.len();
        } else if word(tail.chars().next()) {
            continue;
        }
        out.push_str(&text[copied..at]);
        out.push_str(label);
        copied = end;
        search = end;
    }
    out.push_str(&text[copied..]);
    out
}

/// `text` with each span `find` names replaced by its label. `find(chars, index)` says, for a
/// span that starts at `index`, where it ends and what replaces it.
fn replace_spans<L: Into<Cow<'static, str>>>(
    text: &str,
    find: impl Fn(&[char], usize) -> Option<(usize, L)>,
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        match find(&chars, index).filter(|(end, _)| *end > index) {
            Some((end, label)) => {
                out.push_str(&label.into());
                index = end;
            }
            None => {
                out.push(chars[index]);
                index += 1;
            }
        }
    }
    out
}

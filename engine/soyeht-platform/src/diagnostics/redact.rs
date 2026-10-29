//! Taking out of a line of text what a person should not paste into a public issue: the home
//! folder, the user's name, every path, every link, an address, and anything that looks like a
//! token or a secret.
//!
//! The rules read the text and nothing else, so they are the same on every system and a test
//! feeds them any line. They err toward removing: a relative path of two folders or more, or a
//! long mixed run of letters and digits, is removed even when it was harmless.

use std::path::Path;

const PATH: &str = "<path>";
const URL: &str = "<url>";
const SECRET: &str = "<redacted>";
const EMAIL: &str = "<email>";
const USER: &str = "<user>";

/// A run of at least this many token characters that mixes letters with digits, or upper with
/// lower case, is taken for a key or a token.
const TOKEN_MIN: usize = 24;
/// A name shorter than this is not replaced as a word: it would also cut other words.
const USER_MIN: usize = 3;
/// A field whose name holds one of these words has its value removed.
const SECRET_WORDS: [&str; 10] = [
    "token",
    "secret",
    "password",
    "passwd",
    "authorization",
    "apikey",
    "api_key",
    "cookie",
    "credential",
    "bearer",
];

/// What to remove from a line, besides what the text itself shows.
#[derive(Clone, Debug, Default)]
pub struct Redactor {
    home: Option<String>,
    user: Option<String>,
}

impl Redactor {
    /// A redactor for the person running this program: their home folder and user name.
    pub fn current() -> Self {
        let user = match std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
            Ok(name) if !name.is_empty() => Some(name),
            // Not set, or not text: the name is then only removed where a path shows it.
            _ => None,
        };
        Self::new(std::env::home_dir().as_deref(), user.as_deref())
    }

    pub fn new(home: Option<&Path>, user: Option<&str>) -> Self {
        Self {
            home: home
                .map(|home| {
                    home.to_string_lossy()
                        .trim_end_matches(['/', '\\'])
                        .to_owned()
                })
                .filter(|home| home.len() > 1),
            user: user
                .filter(|user| user.chars().count() >= USER_MIN)
                .map(str::to_owned),
        }
    }

    /// `text` with the home folder, paths, links, secrets, addresses and the user's name taken
    /// out.
    pub fn redact(&self, text: &str) -> String {
        let text = self.without_home(text);
        let text = without_paths(&text);
        let text = without_secret_fields(&text);
        let text = without_tokens(&text);
        let text = without_emails(&text);
        self.without_user(&text)
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

    fn without_user(&self, text: &str) -> String {
        let Some(user) = &self.user else {
            return text.to_owned();
        };
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(user.as_str()) {
            let before = rest[..at].chars().next_back();
            let after = rest[at + user.len()..].chars().next();
            let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
            out.push_str(&rest[..at]);
            out.push_str(if word(before) || word(after) {
                user
            } else {
                USER
            });
            rest = &rest[at + user.len()..];
        }
        out.push_str(rest);
        out
    }
}

/// The characters that end a path or a link in running text.
fn ends_token(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | ',' | ';' | ')' | ']' | '}' | '<' | '>' | '`'
        )
}

/// The characters after which a path or a link may start.
fn starts_token(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '=' | '(' | '[' | '{' | ',' | ':' | ';' | '<' | '>' | '`' | '|'
        )
}

fn is_separator(c: char) -> bool {
    matches!(c, '/' | '\\')
}

/// Where a link or a path that starts at `start` ends, and what it was.
fn token_at(chars: &[char], start: usize) -> Option<(usize, &'static str)> {
    let first = chars[start];
    let next = chars.get(start + 1).copied();
    let quoted = start > 0 && matches!(chars[start - 1], '"' | '\'');
    let token_end = |from: usize| {
        chars[from..]
            .iter()
            .position(|c| ends_token(*c))
            .map_or(chars.len(), |length| from + length)
    };
    // A quoted value may hold blanks inside a folder name, so it ends at its quote.
    let value_end = |from: usize| {
        if quoted {
            let quote = chars[start - 1];
            let mut index = from;
            while index < chars.len() {
                match chars[index] {
                    '\\' => index += 2,
                    c if c == quote => return index,
                    _ => index += 1,
                }
            }
            chars.len()
        } else {
            token_end(from)
        }
    };
    let letters = chars[start..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        .count();
    if letters > 0
        && chars.get(start + letters) == Some(&':')
        && chars.get(start + letters + 1) == Some(&'/')
        && chars.get(start + letters + 2) == Some(&'/')
    {
        return Some((token_end(start + letters), URL));
    }
    let absolute = (first == '/' && next.is_some_and(|c| !c.is_whitespace() && c != '"'))
        || (first == '~' && next.is_some_and(is_separator))
        || (first == '\\' && next == Some('\\'))
        || (first.is_ascii_alphabetic()
            && next == Some(':')
            && chars.get(start + 2).copied().is_some_and(is_separator));
    if absolute {
        return Some((value_end(start + 1), PATH));
    }
    let end = token_end(start);
    let token = &chars[start..end];
    let separators = token.iter().filter(|c| is_separator(**c)).count();
    let relative = separators >= 2
        && token.iter().any(|c| c.is_alphabetic())
        && !is_separator(first)
        && !token.iter().any(|c| matches!(c, '<' | '{' | '='));
    relative.then_some((end, PATH))
}

fn without_paths(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let boundary = index == 0 || starts_token(chars[index - 1]);
        if let Some((end, label)) = boundary
            .then(|| token_at(&chars, index))
            .flatten()
            .filter(|(end, _)| *end > index)
        {
            out.push_str(label);
            index = end;
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

/// The value after a field whose name holds a secret word: `"token":"..."`, `token=...`,
/// `Authorization: Bearer ...`.
fn without_secret_fields(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut search = 0;
    while let Some((at, length)) = SECRET_WORDS
        .iter()
        .filter_map(|word| {
            lower[search..]
                .find(word)
                .map(|found| (search + found, word.len()))
        })
        .min()
    {
        let name_end = find_from(text, at + length, |c| {
            !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        });
        let value_start = find_from(text, name_end, |c| {
            !matches!(c, '"' | '\'' | ' ' | ':' | '=')
        });
        search = name_end;
        // A word with no separator after it is just a word ("the token was refused"), except
        // `Bearer`, which is followed by its token.
        let bearer = &lower[at..at + length] == "bearer";
        if !(bearer || text[name_end..value_start].contains([':', '='])) {
            continue;
        }
        let mut end = find_from(text, value_start, ends_value);
        // `Authorization: Bearer <token>`: the scheme is one more word.
        if matches!(&lower[value_start..end], "bearer" | "basic") {
            let next = find_from(text, end, |c| c != ' ');
            end = find_from(text, next, ends_value);
        }
        if end > value_start {
            out.push_str(&text[copied..value_start]);
            out.push_str(SECRET);
            copied = end;
            search = end;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// The byte offset of the first character at or after `from` for which `stop` holds, else the
/// end of `text`.
fn find_from(text: &str, from: usize, stop: impl Fn(char) -> bool) -> usize {
    text[from..]
        .find(stop)
        .map_or(text.len(), |offset| from + offset)
}

fn ends_value(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '}' | ']')
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '=' | '.')
}

fn looks_like_token(run: &str) -> bool {
    let count = run.chars().count();
    let digit = run.chars().any(|c| c.is_ascii_digit());
    let lower = run.chars().any(|c| c.is_ascii_lowercase());
    let upper = run.chars().any(|c| c.is_ascii_uppercase());
    count >= TOKEN_MIN && ((digit && (lower || upper)) || (lower && upper))
}

fn without_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if looks_like_token(run) {
            out.push_str(SECRET);
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if is_token_char(c) {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

fn without_emails(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        let start = rest[..at]
            .rfind(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '+')))
            .map_or(0, |index| {
                index + rest[index..].chars().next().map_or(1, char::len_utf8)
            });
        let tail = &rest[at + 1..];
        let length = tail
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '-')))
            .unwrap_or(tail.len());
        let domain = tail[..length].trim_end_matches('.');
        if start < at && domain.contains('.') {
            out.push_str(&rest[..start]);
            out.push_str(EMAIL);
            rest = &rest[at + 1 + domain.len()..];
        } else {
            out.push_str(&rest[..=at]);
            rest = &rest[at + 1..];
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Redactor {
        Redactor::new(Some(Path::new("/Users/dana.example")), Some("dana.example"))
    }

    #[test]
    fn a_log_line_loses_its_home_path_and_its_token() {
        let line = r#"{"event":"store.open_failed","path":"/Users/dana.example/Library/Work Files/db.sqlite","token":"s3cr3tValue","note":"sk-9fA2kLm4Qz7Xw1Rt8Yb3Nc5Vd6"}"#;
        let clean = redactor().redact(line);
        for leaked in [
            "dana.example",
            "/Users",
            "Work Files",
            "s3cr3tValue",
            "sk-9fA2kLm4Qz7Xw1Rt8Yb3Nc5Vd6",
        ] {
            assert!(!clean.contains(leaked), "{leaked} leaked: {clean}");
        }
        assert!(clean.contains(r#""event":"store.open_failed""#), "{clean}");
    }

    #[test]
    fn the_name_of_an_event_and_of_a_failure_kind_stay() {
        let line =
            r#"{"event":"engine.reconnect_attempts_exhausted","kind":"ConnectionRefused","n":"3"}"#;
        assert_eq!(redactor().redact(line), line);
    }

    #[test]
    fn paths_of_every_system_are_removed() {
        let redactor = Redactor::new(Some(Path::new(r"C:\Users\dana.example")), None);
        for line in [
            r"opened C:\Users\dana.example\AppData\Local\Soyeht.next\db",
            r#"{"p":"C:\\Users\\dana.example\\AppData"}"#,
            r"share \\server\share\folder\file",
            "start /usr/local/bin/soyeht-engine now",
            "cwd=/home/dana/work/repo",
            "see ~/work/repo please",
            "file:///home/dana/work/file.txt",
            "unit at .config/systemd/user/unit.service",
        ] {
            let clean = redactor.redact(line);
            for leaked in ["dana", "AppData", "usr/local", "systemd", "server", "work"] {
                assert!(
                    !clean.contains(leaked),
                    "{leaked} leaked from {line}: {clean}"
                );
            }
        }
    }

    #[test]
    fn links_addresses_and_secret_fields_are_removed() {
        let redactor = redactor();
        let clean = redactor.redact(
            "GET https://example.test/hook?key=abc123 from dana@example.com \
             Authorization: Bearer abcDEF123456 password=hunter2 api_key: zzz",
        );
        for leaked in [
            "example.test",
            "abc123",
            "dana@example.com",
            "abcDEF123456",
            "hunter2",
            "zzz",
        ] {
            assert!(!clean.contains(leaked), "{leaked} leaked: {clean}");
        }
    }

    #[test]
    fn ordinary_words_that_name_a_secret_stay() {
        let line = "the token was refused and the password prompt closed";
        assert_eq!(redactor().redact(line), line);
    }

    #[test]
    fn a_short_user_name_is_not_cut_out_of_other_words() {
        let redactor = Redactor::new(None, Some("al"));
        assert_eq!(redactor.redact("alpha and al"), "alpha and al");
        let redactor = Redactor::new(None, Some("alex"));
        assert_eq!(
            redactor.redact("alexander and alex"),
            "alexander and <user>"
        );
    }
}

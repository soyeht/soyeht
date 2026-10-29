//! Secrets: the value of a field named like one, runs that look like a token, and addresses of
//! e-mail.

const SECRET: &str = "<redacted>";
const EMAIL: &str = "<email>";

/// A run of at least this many token characters that mixes letters with digits, or upper with
/// lower case, is taken for a key or a token.
const TOKEN_MIN: usize = 24;

/// A field is secret when the last word of its name is one of these (`token`, `authToken`,
/// `x-api-key`, `session`): `sessionCount` and `keyboard` are not, `mapping` is not.
const SECRET_NAMES: [&str; 17] = [
    "token",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "authorization",
    "apikey",
    "cookie",
    "credential",
    "credentials",
    "bearer",
    "pairing",
    "session",
    "private",
    "pin",
    "otp",
    "key",
];
/// `code` is secret only after one of these (`pairingCode`, `verification code`), so `exitCode`
/// is not.
const CODE_PREFIXES: [&str; 9] = [
    "pairing",
    "otp",
    "verification",
    "confirmation",
    "security",
    "login",
    "auth",
    "activation",
    "invite",
];
/// The fewest digits of a number that follows `pin` or `pairing code` in running text.
const CODE_DIGITS_MIN: usize = 4;

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
}

/// The words of a field name, in lower case: split at `_` and `-` and at a capital letter
/// (`pairingCode`, `APIKey`).
fn name_words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut words = vec![String::new()];
    for (index, c) in chars.iter().enumerate() {
        if matches!(c, '_' | '-') {
            words.push(String::new());
            continue;
        }
        let previous = index.checked_sub(1).map(|i| chars[i]);
        let next = chars.get(index + 1);
        let capital_starts = c.is_ascii_uppercase()
            && previous.is_some_and(|p| {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase() && next.is_some_and(char::is_ascii_lowercase))
            });
        if capital_starts {
            words.push(String::new());
        }
        if let Some(word) = words.last_mut() {
            word.push(c.to_ascii_lowercase());
        }
    }
    words.retain(|word| !word.is_empty());
    words
}

/// What a name says about the value after it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Secret {
    No,
    /// The value after a `:` or `=` is removed.
    Field,
    /// A number: it also goes when a space and no separator follow the name (`pin 123456`).
    Number,
}

/// Whether `words` name a secret; `before` is the word that stood before the name in the text,
/// one space away (`verification code`).
fn classify(words: &[String], before: Option<&str>) -> Secret {
    let Some(last) = words.last().map(String::as_str) else {
        return Secret::No;
    };
    let second = words
        .len()
        .checked_sub(2)
        .map(|i| words[i].as_str())
        .or(before);
    if last == "code" {
        return if second.is_some_and(|word| CODE_PREFIXES.contains(&word)) {
            Secret::Number
        } else {
            Secret::No
        };
    }
    match last {
        "pin" | "otp" | "pairing" => Secret::Number,
        _ if SECRET_NAMES.contains(&last) => Secret::Field,
        _ => Secret::No,
    }
}

/// The value after a field whose name ends in a secret word: `"token":"..."`, `token=...`,
/// `Authorization: Bearer ...`, `"pairingCode":"482913"`, and the number after `pin` or
/// `pairing code` in running text. One pass over the line.
pub(super) fn without_secret_fields(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut skip_to = 0;
    let mut at = 0;
    // The end of the name before this one, and its last word.
    let mut before: Option<(usize, String)> = None;
    while let Some(start) = text[at..].find(is_name_char).map(|offset| at + offset) {
        let end = find_from(text, start, |c| !is_name_char(c));
        at = end;
        if start < skip_to {
            continue;
        }
        let words = name_words(&text[start..end]);
        let previous = before
            .take()
            .filter(|(previous_end, _)| text[*previous_end..start] == *" ")
            .map(|(_, word)| word);
        before = words.last().map(|word| (end, word.clone()));
        let kind = classify(&words, previous.as_deref());
        if kind == Secret::No {
            continue;
        }
        let value_start = find_from(text, end, |c| !matches!(c, '"' | '\'' | ' ' | ':' | '='));
        let bearer = words.last().is_some_and(|word| word == "bearer");
        let separated = text[end..value_start].contains([':', '=']);
        let span = if separated || bearer {
            Some(value_end(text, value_start))
        } else if kind == Secret::Number {
            number_after(text, end)
        } else {
            None
        };
        if let Some((from, to)) = span.filter(|(from, to)| to > from) {
            out.push_str(&text[copied..from]);
            out.push_str(SECRET);
            copied = to;
            skip_to = to;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// The value that starts at `start`, as `(start, end)`: a word, or for `Authorization: Bearer x`
/// the word after the scheme.
fn value_end(text: &str, start: usize) -> (usize, usize) {
    let end = find_from(text, start, ends_value);
    if matches!(
        text[start..end].to_ascii_lowercase().as_str(),
        "bearer" | "basic"
    ) {
        let next = find_from(text, end, |c| c != ' ');
        return (start, find_from(text, next, ends_value));
    }
    (start, end)
}

/// The digits of a code written after the name in plain text: `pin 123456`, `pairing code
/// 482913`; the word `code`, `pin` or `number` between is skipped.
fn number_after(text: &str, name_end: usize) -> Option<(usize, usize)> {
    let mut from = find_from(text, name_end, |c| c != ' ');
    let word_end = find_from(text, from, |c| !c.is_ascii_alphabetic());
    if matches!(
        text[from..word_end].to_ascii_lowercase().as_str(),
        "code" | "pin" | "number"
    ) {
        from = find_from(text, word_end, |c| c != ' ');
    }
    let end = find_from(text, from, |c| !c.is_ascii_digit());
    let digits = end - from;
    let alone = !text[end..]
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric);
    (digits >= CODE_DIGITS_MIN && alone).then_some((from, end))
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
    let chars: Vec<char> = run.chars().collect();
    if chars.len() < TOKEN_MIN {
        return false;
    }
    let digit = chars.iter().any(char::is_ascii_digit);
    let lower = chars.iter().any(char::is_ascii_lowercase);
    let upper = chars.iter().any(char::is_ascii_uppercase);
    // A name in camel case (`ConnectionRefusedByTheServer`) has a lower-case letter after every
    // capital; a random key does not.
    let camel = !digit
        && chars
            .windows(2)
            .all(|pair| !pair[0].is_ascii_uppercase() || pair[1].is_ascii_lowercase())
        && chars.last().is_some_and(char::is_ascii_lowercase);
    (digit && (lower || upper)) || (lower && upper && !camel)
}

/// The length of a UUID or a ULID that starts at `start`, the ids of panes and workspaces: they
/// are long mixed runs too, and an issue needs them to tell two panes apart.
fn id_len(chars: &[char], start: usize) -> Option<usize> {
    const UUID: usize = 36;
    const ULID: usize = 26;
    let uuid = chars.get(start..start + UUID).filter(|id| {
        id.iter().enumerate().all(|(index, c)| match index {
            8 | 13 | 18 | 23 => *c == '-',
            _ => c.is_ascii_hexdigit(),
        })
    });
    if uuid.is_some() {
        return Some(UUID);
    }
    // The alphabet of a ULID leaves out I, L, O and U; its first digit is at most 7.
    chars
        .get(start..start + ULID)
        .filter(|id| {
            matches!(id[0], '0'..='7')
                && id
                    .iter()
                    .all(|c| c.is_ascii_digit() || (c.is_ascii_uppercase() && !"ILOU".contains(*c)))
        })
        .map(|_| ULID)
}

/// The beginnings of the keys that services hand out (`sk-...`, `ghp_...`). An id-shaped value
/// glued to one is a key, not a pane's id.
const KEY_PREFIXES: [&str; 14] = [
    "sk", "pk", "rk", "ghp", "gho", "ghu", "ghs", "ghr", "xoxb", "xoxp", "xoxa", "glpat", "npm",
    "pat",
];

/// Whether the id that starts at `start` follows a key prefix and a `-` or `_`.
fn glued_to_key_prefix(chars: &[char], start: usize) -> bool {
    let Some(before) = start
        .checked_sub(1)
        .filter(|i| matches!(chars[*i], '-' | '_'))
    else {
        return false;
    };
    let word_start = chars[..before]
        .iter()
        .rposition(|c| !c.is_ascii_alphanumeric())
        .map_or(0, |i| i + 1);
    let word: String = chars[word_start..before].iter().collect();
    KEY_PREFIXES.contains(&word.to_ascii_lowercase().as_str())
}

/// Whether `chars[start..start + length]` stands alone: no token character on either side.
fn stands_alone(chars: &[char], start: usize, length: usize) -> bool {
    let before = start.checked_sub(1).and_then(|index| chars.get(index));
    let after = chars.get(start + length);
    !before.is_some_and(|c| c.is_ascii_alphanumeric())
        && !after.is_some_and(|c| c.is_ascii_alphanumeric())
}

pub(super) fn without_tokens(text: &str) -> String {
    super::replace_spans(text, |chars, index| {
        let starts_run =
            is_token_char(chars[index]) && (index == 0 || !is_token_char(chars[index - 1]));
        starts_run.then(|| {
            let end = index
                + chars[index..]
                    .iter()
                    .take_while(|c| is_token_char(**c))
                    .count();
            (end, without_tokens_in_run(&chars[index..end]))
        })
    })
}

/// A run of token characters with its ids kept and what is around them judged on its own.
fn without_tokens_in_run(run: &[char]) -> String {
    let mut out = String::new();
    let mut piece = 0;
    let mut cursor = 0;
    while cursor < run.len() {
        match id_len(run, cursor).filter(|length| {
            stands_alone(run, cursor, *length) && !glued_to_key_prefix(run, cursor)
        }) {
            Some(length) => {
                push_run(&mut out, &run[piece..cursor]);
                out.extend(&run[cursor..cursor + length]);
                cursor += length;
                piece = cursor;
            }
            None => cursor += 1,
        }
    }
    push_run(&mut out, &run[piece..]);
    out
}

fn push_run(out: &mut String, run: &[char]) {
    let text: String = run.iter().collect();
    out.push_str(if looks_like_token(&text) {
        SECRET
    } else {
        &text
    });
}

pub(super) fn without_emails(text: &str) -> String {
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

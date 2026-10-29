//! Secrets: the value of a field named like one, runs that look like a token, and addresses of
//! e-mail.

const SECRET: &str = "<redacted>";
const EMAIL: &str = "<email>";

/// A run of at least this many token characters that mixes letters with digits, or upper with
/// lower case, is taken for a key or a token.
const TOKEN_MIN: usize = 24;

/// A field whose name holds one of these words has its value removed. A word of
/// [`SHORT_WORD`] letters or fewer ("pin", "key") must start a word of the name (after a
/// separator or at a capital: `apiKey`, `x-api-key`), so "mapping" and "monkey" do not match.
const SECRET_WORDS: [&str; 18] = [
    "token",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "authorization",
    "apikey",
    "api_key",
    "cookie",
    "credential",
    "bearer",
    "pairing",
    "session",
    "private",
    "pin",
    "otp",
    "code",
    "key",
];
const SHORT_WORD: usize = 4;

/// Where a name's word starts: at the start, after a character that is not a letter or a digit,
/// or at a capital letter after a lower-case one.
fn starts_word(text: &str, at: usize) -> bool {
    let mut before = text[..at].chars().rev();
    match before.next() {
        None => true,
        Some(previous) => {
            !previous.is_ascii_alphanumeric()
                || (previous.is_ascii_lowercase()
                    && text[at..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase()))
        }
    }
}

/// Every place a secret word occurs in `text`, in order. `lower` is `text` in ASCII lower case.
fn occurrences(text: &str, lower: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for word in SECRET_WORDS {
        let mut from = 0;
        while let Some(at) = lower[from..].find(word) {
            let at = from + at;
            from = at + word.len();
            if word.len() > SHORT_WORD || starts_word(text, at) {
                found.push((at, word.len()));
            }
        }
    }
    found.sort_unstable();
    found
}

/// The value after a field whose name holds a secret word: `"token":"..."`, `token=...`,
/// `Authorization: Bearer ...`, `"pairingCode":"482913"`. One pass over the line.
pub(super) fn without_secret_fields(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut skip_to = 0;
    for (at, length) in occurrences(text, &lower) {
        if at < skip_to {
            continue;
        }
        let name_end = find_from(text, at + length, |c| {
            !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        });
        let value_start = find_from(text, name_end, |c| {
            !matches!(c, '"' | '\'' | ' ' | ':' | '=')
        });
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
            skip_to = end;
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
        match id_len(run, cursor).filter(|length| stands_alone(run, cursor, *length)) {
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

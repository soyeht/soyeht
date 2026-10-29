//! Paths and links.

const PATH: &str = "<path>";
const URL: &str = "<url>";

/// The characters that end a link or a relative path in running text.
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

/// Where an absolute path that is not quoted ends. A folder name may hold blanks ("My
/// Projects"), so the path runs through them to a character that cannot be part of a path in
/// text: a quote or bracket, a comma, a new option (` -x`), `: ` and a letter, or the end of the
/// line. It may take words after the path with it; an issue loses a word and no folder name.
fn absolute_end(chars: &[char], from: usize) -> usize {
    let mut end = from;
    while let Some(c) = chars.get(end) {
        let stops = matches!(
            c,
            '"' | '\'' | ')' | ']' | '}' | '<' | '>' | '`' | ',' | ';' | '\n'
        ) || (*c == ':'
            && chars.get(end + 1) == Some(&' ')
            && chars.get(end + 2).is_some_and(|next| next.is_alphabetic()))
            || (*c == ' ' && chars.get(end + 1) == Some(&'-'));
        if stops {
            break;
        }
        end += 1;
    }
    while end > from && chars[end - 1] == ' ' {
        end -= 1;
    }
    end
}

/// Where the value that starts after `quote`'s opening, at `from`, ends: at the closing quote.
fn quoted_end(chars: &[char], from: usize, quote: char) -> usize {
    let mut index = from;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 2,
            c if c == quote => return index,
            _ => index += 1,
        }
    }
    chars.len()
}

/// Where a link or a path that starts at `start` ends, and what it was.
fn token_at(chars: &[char], start: usize) -> Option<(usize, &'static str)> {
    let first = chars[start];
    let next = chars.get(start + 1).copied();
    let quote = (start > 0 && matches!(chars[start - 1], '"' | '\'')).then(|| chars[start - 1]);
    let token_end = |from: usize| {
        chars[from..]
            .iter()
            .position(|c| ends_token(*c))
            .map_or(chars.len(), |length| from + length)
    };
    let letters = chars[start..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        .count();
    if letters > 0 && chars[start + letters..].starts_with(&[':', '/', '/']) {
        return Some((token_end(start + letters), URL));
    }
    let absolute = (first == '/' && next.is_some_and(|c| !c.is_whitespace() && c != '"'))
        || (first == '~' && next.is_some_and(is_separator))
        || (first == '\\' && next == Some('\\'))
        || (first.is_ascii_alphabetic()
            && next == Some(':')
            && chars.get(start + 2).copied().is_some_and(is_separator));
    if absolute {
        let end = match quote {
            Some(quote) => quoted_end(chars, start + 1, quote),
            None => absolute_end(chars, start + 1),
        };
        return Some((end, PATH));
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

pub(super) fn without_paths(text: &str) -> String {
    super::replace_spans(text, |chars, index| {
        let boundary = index == 0 || starts_token(chars[index - 1]);
        boundary.then(|| token_at(chars, index)).flatten()
    })
}

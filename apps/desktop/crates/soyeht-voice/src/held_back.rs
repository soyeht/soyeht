//! What an unfinished construct at the end of a reply that is still arriving would still change,
//! so that the text spoken so far is always the start of the text to come (contract 20, section 3).

use crate::text::{is_letter, is_number};

fn is_word_character(character: char) -> bool {
    is_letter(character) || is_number(character)
}

/// The markdown cut where an unfinished link, star or underscore begins in its last line. Inside an
/// open code fence nothing is held: code is not read.
pub(crate) fn held_back(markdown: &str) -> &str {
    let fences = markdown
        .split('\n')
        .filter(|line| line.starts_with("```") || line.starts_with("~~~"))
        .count();
    if fences % 2 != 0 {
        return markdown;
    }
    let line_start = markdown.rfind('\n').map_or(0, |index| index + 1);
    let line: Vec<(usize, char)> = markdown[line_start..]
        .char_indices()
        .map(|(offset, character)| (line_start + offset, character))
        .collect();
    match first_open(&line) {
        Some(cut) => &markdown[..cut],
        None => markdown,
    }
}

/// The byte offset of the earliest of: a `[` still open, a lone `*`, a lone `_`.
fn first_open(line: &[(usize, char)]) -> Option<usize> {
    let mut open: Option<usize> = None;
    let mut star: Option<usize> = None;
    let mut underscore: Option<usize> = None;
    let mut in_code = false;
    let mut previous = '\n';
    let mut position = 0;
    while position < line.len() {
        let (offset, character) = line[position];
        let following = line.get(position + 1).map_or('\n', |&(_, next)| next);
        match character {
            '`' => in_code = !in_code,
            '[' if !in_code => open = open.or(Some(offset)),
            ']' if !in_code => {
                if following == '(' {
                    if let Some(close) = line[position + 1..]
                        .iter()
                        .position(|&(_, next)| next == ')')
                    {
                        position += 1 + close;
                        open = None;
                    }
                } else {
                    open = None;
                }
            }
            '*' if !in_code => {
                if following == '*' {
                    position += 1;
                } else if !(following.is_whitespace() && previous.is_whitespace()) {
                    // A bullet, or a star set apart by blanks, is not emphasis.
                    star = star.xor(Some(offset));
                }
            }
            '_' if !in_code => {
                if following == '_' {
                    position += 1;
                } else if (!is_word_character(previous) || !is_word_character(following))
                    && !(following.is_whitespace() && previous.is_whitespace())
                {
                    // An underscore inside a word (snake_case) is not emphasis.
                    underscore = underscore.xor(Some(offset));
                }
            }
            _ => {}
        }
        previous = line[position].1;
        position += 1;
    }
    [open, star, underscore].into_iter().flatten().min()
}

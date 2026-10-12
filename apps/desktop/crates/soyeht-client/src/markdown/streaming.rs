//! Closes what a reply that is still arriving has left open.
//!
//! "Here is **the" would show its asterisks until the closing pair arrives, then jump to
//! bold. Closing the open span first shows the style as the words are typed. Only the spans
//! whose delimiters cannot be mistaken for text are tracked: code spans (a run of backticks),
//! bold (`**`) and strikethrough (`~~`). A delimiter opens a span when the next character
//! is not whitespace and closes one when the previous character is not whitespace, as in
//! CommonMark; one that ends the text, with nothing after it yet, is dropped. An open code
//! fence is left alone (the parser already runs it to the end), and a blank line ends every
//! open span. `contracts/vectors/markdown.json` holds the cases the Swift client shares.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delimiter {
    Code(usize),
    Bold,
    Strike,
}

impl Delimiter {
    fn text(self) -> String {
        match self {
            Self::Code(length) => "`".repeat(length),
            Self::Bold => "**".to_owned(),
            Self::Strike => "~~".to_owned(),
        }
    }
}

/// The text with its open spans closed.
pub(super) fn close(text: &str) -> String {
    let mut open: Vec<(Delimiter, usize)> = Vec::new();
    let mut fence: Option<char> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim_start();
        let marker = ['`', '~']
            .into_iter()
            .find(|&c| trimmed.starts_with(&c.to_string().repeat(3)));
        match (fence, marker) {
            (Some(opened), Some(marker)) if opened == marker => {
                if trimmed.trim_start_matches(marker).trim().is_empty() {
                    fence = None;
                }
            }
            (Some(_), _) => {}
            (None, Some(marker)) => {
                open.clear();
                fence = Some(marker);
            }
            (None, None) if trimmed.is_empty() => open.clear(),
            (None, None) => scan(line, start, &mut open),
        }
    }
    if fence.is_some() {
        return text.to_owned();
    }
    let mut closed = text.to_owned();
    if let Some(&(delimiter, begin)) = open.last()
        && begin + delimiter.text().len() == text.len()
    {
        closed.truncate(begin);
        open.pop();
    }
    for (delimiter, _) in open.iter().rev() {
        closed.push_str(&delimiter.text());
    }
    closed
}

/// Tracks the delimiters of one line; `base` is the line's offset in the whole text.
fn scan(line: &str, base: usize, open: &mut Vec<(Delimiter, usize)>) {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut index = 0;
    while index < chars.len() {
        let (position, current) = chars[index];
        let run = chars[index..]
            .iter()
            .take_while(|&&(_, c)| c == current)
            .count();
        let before = index.checked_sub(1).map(|i| chars[i].1);
        let after = chars.get(index + run).map(|&(_, c)| c);
        let in_code = matches!(open.last(), Some((Delimiter::Code(_), _)));
        index += match current {
            '`' => {
                match open.last() {
                    Some(&(Delimiter::Code(length), _)) => {
                        if length == run {
                            open.pop();
                        }
                    }
                    _ => open.push((Delimiter::Code(run), base + position)),
                }
                run
            }
            _ if in_code => 1,
            '\\' => 2,
            '*' | '~' if run == 2 => {
                let delimiter = if current == '*' {
                    Delimiter::Bold
                } else {
                    Delimiter::Strike
                };
                toggle(open, delimiter, base + position, before, after);
                run
            }
            _ => run,
        };
    }
}

fn toggle(
    open: &mut Vec<(Delimiter, usize)>,
    delimiter: Delimiter,
    position: usize,
    before: Option<char>,
    after: Option<char>,
) {
    if let Some(index) = open.iter().rposition(|&(d, _)| d == delimiter) {
        if before.is_some_and(|c| !c.is_whitespace()) {
            open.remove(index);
        }
    } else if after.is_none_or(|c| !c.is_whitespace()) {
        open.push((delimiter, position));
    }
}

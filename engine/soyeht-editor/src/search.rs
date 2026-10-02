use crate::{Error, Result};
use memchr::{memchr_iter, memmem};
use regex::RegexBuilder;
use soyeht_protocol::{TextPosition, TextRange};

const PREVIEW_CHARACTERS: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct ContentMatch {
    pub range: TextRange,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContentSearch {
    pub matches: Vec<ContentMatch>,
    pub replacement: Option<String>,
    pub replaced: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathSearchMatch {
    pub path: String,
    pub score: i64,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy)]
struct ByteMatch {
    start: usize,
    end: usize,
}

/// How a query matches text: exact or case-folded, anywhere or on word
/// boundaries, literally or as a regular expression.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

pub struct ContentMatcher {
    pattern: ContentPattern,
    options: MatchOptions,
}

enum ContentPattern {
    Sensitive(memmem::Finder<'static>),
    Expression(regex::Regex),
}

pub fn search_content(
    text: &str,
    query: &str,
    replacement: Option<&str>,
    case_sensitive: bool,
    limit: usize,
) -> Result<ContentSearch> {
    ContentMatcher::new(query, case_sensitive)?.search(text, replacement, limit)
}

impl ContentMatcher {
    pub fn new(query: &str, case_sensitive: bool) -> Result<Self> {
        Self::with_options(
            query,
            MatchOptions {
                case_sensitive,
                ..MatchOptions::default()
            },
        )
    }

    pub fn with_options(query: &str, options: MatchOptions) -> Result<Self> {
        if query.is_empty() {
            return Err(Error::EmptySearch);
        }
        let pattern = if options.case_sensitive && !options.regex {
            ContentPattern::Sensitive(memmem::Finder::new(query.as_bytes()).into_owned())
        } else {
            let source = if options.regex {
                query.to_owned()
            } else {
                regex::escape(query)
            };
            ContentPattern::Expression(
                RegexBuilder::new(&source)
                    .case_insensitive(!options.case_sensitive)
                    .multi_line(true)
                    .unicode(true)
                    .build()
                    .map_err(|_| Error::SearchPattern)?,
            )
        };
        Ok(Self { pattern, options })
    }

    /// Every non-empty match in `text` as byte offsets, in document order.
    pub fn byte_matches<'t>(&'t self, text: &'t str) -> impl Iterator<Item = (usize, usize)> + 't {
        let found: Box<dyn Iterator<Item = (usize, usize)> + 't> = match &self.pattern {
            ContentPattern::Sensitive(finder) => {
                let length = finder.needle().len();
                Box::new(
                    finder
                        .find_iter(text.as_bytes())
                        .map(move |start| (start, start + length)),
                )
            }
            ContentPattern::Expression(expression) => Box::new(
                expression
                    .find_iter(text)
                    .map(|found| (found.start(), found.end())),
            ),
        };
        let whole_word = self.options.whole_word;
        found.filter(move |(start, end)| {
            start < end && (!whole_word || on_word_boundaries(text, *start, *end))
        })
    }

    /// Whether `start..end` is a match of this query, judged in the context
    /// of the whole text so anchors and word boundaries hold.
    pub fn matches_at(&self, text: &str, start: usize, end: usize) -> bool {
        let Some(slice) = text.get(start..end) else {
            return false;
        };
        let exact = match &self.pattern {
            ContentPattern::Sensitive(finder) => slice.as_bytes() == finder.needle(),
            ContentPattern::Expression(expression) => expression
                .find_at(text, start)
                .is_some_and(|found| found.start() == start && found.end() == end),
        };
        exact && start < end && (!self.options.whole_word || on_word_boundaries(text, start, end))
    }

    /// The text that replaces the match starting at `start`: the replacement
    /// itself, or for a regular expression with `$1` and `${name}` expanded.
    pub fn expand(&self, text: &str, start: usize, replacement: &str) -> String {
        if let ContentPattern::Expression(expression) = &self.pattern
            && self.options.regex
            && let Some(captures) = expression.captures_at(text, start)
        {
            let mut expanded = String::new();
            captures.expand(replacement, &mut expanded);
            return expanded;
        }
        replacement.to_owned()
    }

    pub fn search(
        &self,
        text: &str,
        replacement: Option<&str>,
        limit: usize,
    ) -> Result<ContentSearch> {
        let limit = limit.min(soyeht_protocol::limits::EDITOR_SEARCH_RESULTS);
        let byte_matches = self.find_matches(text, limit);
        let lines = line_starts(text);
        let matches = byte_matches
            .iter()
            .map(|found| ContentMatch {
                range: text_range(text, &lines, *found),
                preview: preview(text, &lines, found.start),
            })
            .collect();
        let updated = replacement
            .map(|value| replace_matches(text, &byte_matches, value))
            .transpose()?;
        Ok(ContentSearch {
            matches,
            replacement: updated,
            replaced: replacement.map_or(0, |_| byte_matches.len()),
        })
    }

    fn find_matches(&self, text: &str, limit: usize) -> Vec<ByteMatch> {
        self.byte_matches(text)
            .take(limit)
            .map(|(start, end)| ByteMatch { start, end })
            .collect()
    }
}

fn on_word_boundaries(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(is_word_character) && !after.is_some_and(is_word_character)
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

pub fn rank_paths(paths: &[String], query: &str, limit: usize) -> Result<Vec<PathSearchMatch>> {
    if query.is_empty() {
        return Err(Error::EmptySearch);
    }
    let query = query
        .chars()
        .map(|character| character.to_lowercase().collect::<String>())
        .collect::<Vec<_>>();
    let mut matches = paths
        .iter()
        .filter_map(|path| rank_path(path, &query))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.chars().count().cmp(&right.path.chars().count()))
            .then_with(|| left.path.cmp(&right.path))
    });
    matches.truncate(limit.min(soyeht_protocol::limits::EDITOR_SEARCH_RESULTS));
    Ok(matches)
}

fn replace_matches(text: &str, matches: &[ByteMatch], replacement: &str) -> Result<String> {
    let removed = matches
        .iter()
        .map(|found| found.end.saturating_sub(found.start))
        .sum::<usize>();
    let added = replacement
        .len()
        .checked_mul(matches.len())
        .ok_or(Error::EditTooLarge)?;
    let capacity = text
        .len()
        .checked_sub(removed)
        .and_then(|size| size.checked_add(added))
        .ok_or(Error::EditTooLarge)?;
    if capacity > soyeht_protocol::limits::EDITOR_HISTORY_BYTES {
        return Err(Error::EditTooLarge);
    }
    let mut output = String::with_capacity(capacity);
    let mut cursor = 0;
    for found in matches {
        output.push_str(&text[cursor..found.start]);
        output.push_str(replacement);
        cursor = found.end;
    }
    output.push_str(&text[cursor..]);
    Ok(output)
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(memchr_iter(b'\n', text.as_bytes()).map(|index| index + 1))
        .collect()
}

fn text_range(text: &str, lines: &[usize], found: ByteMatch) -> TextRange {
    TextRange {
        start: text_position(text, lines, found.start),
        end: text_position(text, lines, found.end),
    }
}

fn text_position(text: &str, lines: &[usize], byte: usize) -> TextPosition {
    let line = lines
        .partition_point(|start| *start <= byte)
        .saturating_sub(1);
    let line_start = lines.get(line).copied().unwrap_or(0);
    let column = text[line_start..byte].encode_utf16().count();
    TextPosition {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        column: u32::try_from(column).unwrap_or(u32::MAX),
    }
}

fn preview(text: &str, lines: &[usize], byte: usize) -> String {
    let line = lines
        .partition_point(|start| *start <= byte)
        .saturating_sub(1);
    let start = lines.get(line).copied().unwrap_or(0);
    let end = lines.get(line + 1).copied().unwrap_or(text.len());
    let value = text[start..end].trim_end_matches(['\r', '\n']);
    if value.chars().count() <= PREVIEW_CHARACTERS {
        return value.to_owned();
    }
    let relative = byte.saturating_sub(start);
    let match_character = value[..relative.min(value.len())].chars().count();
    let before = PREVIEW_CHARACTERS / 2;
    let first = match_character.saturating_sub(before);
    value.chars().skip(first).take(PREVIEW_CHARACTERS).collect()
}

fn rank_path(path: &str, query: &[String]) -> Option<PathSearchMatch> {
    let characters = path.char_indices().collect::<Vec<_>>();
    let mut cursor = 0;
    let mut indices = Vec::with_capacity(query.len());
    for query_character in query {
        let relative = characters[cursor..].iter().position(|(_, character)| {
            character.to_lowercase().collect::<String>() == *query_character
        })?;
        let index = cursor + relative;
        indices.push(index);
        cursor = index + 1;
    }
    let score = path_score(path, &characters, &indices);
    let first = *indices.first()?;
    let last = *indices.last()?;
    let start_column = path[..characters[first].0].encode_utf16().count();
    let last_end = characters[last].0 + characters[last].1.len_utf8();
    let end_column = path[..last_end].encode_utf16().count();
    Some(PathSearchMatch {
        path: path.to_owned(),
        score,
        range: TextRange {
            start: TextPosition {
                line: 0,
                column: u32::try_from(start_column).unwrap_or(u32::MAX),
            },
            end: TextPosition {
                line: 0,
                column: u32::try_from(end_column).unwrap_or(u32::MAX),
            },
        },
    })
}

fn path_score(path: &str, characters: &[(usize, char)], indices: &[usize]) -> i64 {
    let mut score = 0_i64;
    for (position, index) in indices.iter().copied().enumerate() {
        score += 10;
        if position > 0 && indices[position - 1] + 1 == index {
            score += 15;
        }
        if index == 0 || is_separator(characters[index - 1].1) {
            score += 20;
        }
        score += i64::try_from(50_usize.saturating_sub(index)).unwrap_or(0);
    }
    score - i64::try_from(path.chars().count()).unwrap_or(i64::MAX)
}

fn is_separator(character: char) -> bool {
    matches!(character, '/' | '\\' | '-' | '_' | '.' | ' ')
}

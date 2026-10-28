//! Subagents in the transcript: each one's messages nest under its row, so nothing a subagent
//! does reads as the parent agent's own.

use crate::{agent::elapsed_text, timestamp};
use soyeht_protocol::{SubagentRun, SubagentStatus, TranscriptMessage};
use std::{collections::HashMap, time::Duration};

/// How long the one-line preview of a subagent's last message may be.
const PREVIEW_LIMIT: usize = 160;
/// What a subagent whose row has not arrived is called.
const UNNAMED: &str = "Subagent";

/// One entry of a displayed transcript: a message, or a subagent row holding the activity that
/// subagent owns.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry<'a> {
    Message(&'a TranscriptMessage),
    Subagent(Group<'a>),
}

impl Entry<'_> {
    pub fn id(&self) -> String {
        match self {
            Self::Message(message) => message.local_id.clone(),
            Self::Subagent(group) => format!("subagent:{}", group.subagent_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group<'a> {
    pub subagent_id: &'a str,
    /// The subagent's row; absent while only a later transcript page is loaded.
    pub run: Option<&'a SubagentRun>,
    /// The `localId` of the message that holds `run`.
    pub row: Option<&'a str>,
    pub entries: Vec<Entry<'a>>,
}

/// Nests each subagent's messages under its row.
pub fn entries(messages: &[TranscriptMessage]) -> Vec<Entry<'_>> {
    let mut owned: HashMap<&str, Vec<&TranscriptMessage>> = HashMap::new();
    for message in messages {
        if let Some(owner) = &message.subagent_id {
            owned.entry(owner).or_default().push(message);
        }
    }
    let rows: Vec<&str> = messages
        .iter()
        .filter_map(|message| message.subagent.as_ref())
        .map(|run| run.subagent_id.as_str())
        .collect();
    let mut placed: Vec<&str> = Vec::new();
    let mut result = Vec::new();
    for message in messages {
        if let Some(owner) = &message.subagent_id {
            // A page can hold a subagent's activity without the row it follows.
            if !rows.contains(&owner.as_str()) && !placed.contains(&owner.as_str()) {
                placed.push(owner);
                result.push(Entry::Subagent(group(owner, None, &owned, &mut Vec::new())));
            }
        } else if let Some(run) = &message.subagent {
            result.push(Entry::Subagent(group(
                &run.subagent_id,
                Some((&message.local_id, run)),
                &owned,
                &mut Vec::new(),
            )));
        } else {
            result.push(Entry::Message(message));
        }
    }
    result
}

/// `open` holds the subagents above this one, so a transcript that names a subagent inside
/// itself cannot nest for ever.
fn group<'a>(
    id: &'a str,
    row: Option<(&'a str, &'a SubagentRun)>,
    owned: &HashMap<&'a str, Vec<&'a TranscriptMessage>>,
    open: &mut Vec<&'a str>,
) -> Group<'a> {
    open.push(id);
    let entries = owned
        .get(id)
        .into_iter()
        .flatten()
        .filter_map(|message| match &message.subagent {
            Some(nested) if open.contains(&nested.subagent_id.as_str()) => None,
            Some(nested) => Some(Entry::Subagent(group(
                &nested.subagent_id,
                Some((&message.local_id, nested)),
                owned,
                open,
            ))),
            None => Some(Entry::Message(message)),
        })
        .collect();
    open.pop();
    Group {
        subagent_id: id,
        run: row.map(|(_, run)| run),
        row: row.map(|(local_id, _)| local_id),
        entries,
    }
}

/// How many subagents the parent is waiting on: the running rows that end the transcript, before
/// the parent says anything else.
pub fn awaited(entries: &[Entry<'_>]) -> usize {
    entries
        .iter()
        .rev()
        .map_while(|entry| match entry {
            Entry::Subagent(group) => Some(group),
            Entry::Message(_) => None,
        })
        .filter(|group| {
            group
                .run
                .is_some_and(|run| run.status == SubagentStatus::Running)
        })
        .count()
}

/// The subtle status shown while the parent waits on its subagents.
pub fn waiting(count: usize) -> String {
    format!(
        "Waiting for {count} {}",
        if count == 1 { "subagent" } else { "subagents" }
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Succeeded,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentPresentation {
    pub name: String,
    pub model: Option<String>,
    pub status: Status,
    /// The elapsed time while it runs, else how it ended.
    pub trailing: String,
    /// The one-line preview of what it reported.
    pub summary: Option<String>,
}

impl SubagentPresentation {
    /// `now` is seconds since the epoch.
    pub fn new(run: Option<&SubagentRun>, now: f64) -> Self {
        let (status, trailing) = match run.map(|run| &run.status) {
            Some(SubagentStatus::Running) => (
                Status::Running,
                run.and_then(|run| run.started_at.as_deref())
                    .and_then(timestamp::seconds_since_epoch)
                    .map_or_else(
                        || "running".to_owned(),
                        |started| {
                            elapsed_text(Duration::from_secs((now - started).max(0.0) as u64))
                        },
                    ),
            ),
            Some(SubagentStatus::Completed) => (Status::Succeeded, "done".to_owned()),
            Some(SubagentStatus::Failed) => (Status::Failed, "failed".to_owned()),
            Some(SubagentStatus::Disconnected) => (Status::Failed, "disconnected".to_owned()),
            Some(SubagentStatus::Cancelled) => (Status::Stopped, "cancelled".to_owned()),
            None => (Status::Stopped, String::new()),
        };
        Self {
            name: run.map_or(UNNAMED, |run| run.name.as_str()).to_owned(),
            model: run.and_then(|run| run.model.clone()),
            status,
            trailing,
            summary: run
                .and_then(|run| run.summary.as_deref())
                .and_then(|summary| preview(summary, PREVIEW_LIMIT)),
        }
    }
}

/// The collapsed row's preview of a subagent's last message: its meaningful content on one line.
/// Lead-in lines ending in ":" ("Contents verbatim:"), code fences, list markers and runs of
/// whitespace are dropped; lines are joined with " · " and the result is cut at a word with an
/// ellipsis.
pub fn preview(summary: &str, limit: usize) -> Option<String> {
    let lines: Vec<String> = summary.split(is_line_break).filter_map(line).collect();
    let content: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| !line.ends_with(':'))
        .collect();
    let joined = if content.is_empty() {
        lines.join(" \u{b7} ")
    } else {
        content.join(" \u{b7} ")
    };
    (!joined.is_empty()).then(|| truncated(&joined, limit))
}

fn is_line_break(character: char) -> bool {
    matches!(
        character,
        '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

fn line(raw: &str) -> Option<String> {
    let mut text = raw.trim_matches(is_inline_space);
    if text.starts_with("```") || text.starts_with("~~~") {
        return None;
    }
    for marker in ["- ", "* ", "+ ", "\u{2022} "] {
        text = text.strip_prefix(marker).unwrap_or(text);
    }
    if text.starts_with('#') {
        text = text.trim_start_matches('#');
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// Whitespace within a line: Swift's `CharacterSet.whitespaces`.
fn is_inline_space(character: char) -> bool {
    character == '\t' || (character.is_whitespace() && !is_line_break(character))
}

fn truncated(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(limit.saturating_sub(1)).collect();
    // Prefer ending at a word unless that drops most of the line.
    if let Some(space) = cut.rfind(' ')
        && cut[..space].chars().count() > limit / 2
    {
        cut.truncate(space);
    }
    let trimmed =
        cut.trim_matches(|character: char| is_inline_space(character) || is_punctuation(character));
    format!(
        "{}\u{2026}",
        if trimmed.is_empty() { &cut } else { trimmed }
    )
}

/// Unicode punctuation (general categories P*), as Swift's `CharacterSet.punctuationCharacters`
/// has it, for the characters that end a cut line: the ASCII ones, Latin-1's and the general
/// punctuation block's.
fn is_punctuation(character: char) -> bool {
    // `\u{00bb}` is written in full form so the vocabulary scan does not misread it.
    matches!(
        character,
        '!' | '"' | '#' | '%' | '&' | '\'' | '(' | ')' | '*' | ',' | '-' | '.' | '/' | ':' | ';'
            | '?' | '@' | '[' | '\\' | ']' | '_' | '{' | '}'
            | '\u{a1}' | '\u{a7}' | '\u{ab}' | '\u{b6}' | '\u{b7}' | '\u{00bb}' | '\u{bf}'
            | '\u{2010}'..='\u{2027}'
            | '\u{2030}'..='\u{205e}'
            | '\u{3001}'..='\u{3003}'
    )
}

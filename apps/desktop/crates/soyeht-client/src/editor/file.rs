//! One open file: its page, the caret, and the edits on their way to the engine.
//!
//! A keystroke changes the page at once and joins a queue; the queue goes to the engine one write
//! at a time, each to the revision the one before made. Nothing else is read from the engine while
//! the queue is not empty: a page read then would not have the keystrokes yet. What was asked in
//! the meantime waits and is done when the queue is empty.

mod held;

use super::at_least;
use super::{
    commands::{EditorCommand, PageRequest},
    editing::Plan,
    page::{Page, position},
    selection::{Motion, Selection, clamped, target},
    viewport::{PAGE_LINES, Window, landing_start},
};
use crate::{Command, Effect};
use soyeht_protocol::{
    FileAuthor, FileDocument, FileFindQuery, FileFindResult, FileReadResult, FileUndoResult,
    FileWriteResult, TextEdit, TextPosition, TextRange,
};
use std::collections::VecDeque;

pub use held::Action;

/// What waits for the edits on their way to the engine.
#[derive(Debug, Clone, PartialEq)]
enum Deferred {
    Reload,
    Save,
    Replay { redo: bool },
    Close,
}

/// The search the replica last asked for, and what it found.
#[derive(Debug, Clone, Default)]
pub struct Search {
    seq: u64,
    /// Whether the answer takes the selection to the match.
    navigate: bool,
    /// The search again once the page that holds the match has been read, for the matches on it.
    again: Option<(FileFindQuery, bool)>,
    pub result: Option<FileFindResult>,
}

#[derive(Debug, Clone)]
pub struct EditorFile {
    pub doc: FileDocument,
    pub page: Option<Page>,
    pub selection: Selection,
    /// The lines of the document: the engine's count, moved by what was typed since.
    pub lines: u32,
    pub changed_lines: Vec<u32>,
    pub saved_by: Option<FileAuthor>,
    pub search: Search,
    queue: VecDeque<Vec<TextEdit>>,
    in_flight: bool,
    /// The first line of the page being read.
    reading: Option<u32>,
    /// Where the caret goes once the page that holds it has been read, and the first line of
    /// that page: a page read earlier does not take it.
    pending: Option<(u32, Selection)>,
    /// What the person did while the caret was on its way: it is done at the target, in order,
    /// when the page that holds it has been read (`file/held.rs`).
    held: VecDeque<Action>,
    /// An undo or redo is on its way to the engine: the page it leaves is not the one later
    /// actions are for, so they wait for its answer like they wait for a jump.
    replaying: bool,
    /// Whether the held actions already overflowed their bound (logged once per landing).
    overflowed: bool,
    /// How many of the person's actions were dropped, for the view to say so until the next one.
    pub dropped_keys: u32,
    deferred: Vec<Deferred>,
    reveal: bool,
    /// The first line in view and how many are, as the view last said.
    view: (u32, u32),
}

fn send(command: EditorCommand) -> Effect {
    Effect::Send(Command::Editor(command))
}

/// How many lines a text adds to the document.
fn added_lines(text: &str, removed: u32) -> i64 {
    i64::try_from(text.matches('\n').count()).unwrap_or(i64::MAX) - i64::from(removed)
}

impl EditorFile {
    pub fn new(doc: FileDocument) -> Self {
        Self {
            lines: doc.line_count,
            doc,
            page: None,
            selection: Selection::caret(position(0, 0)),
            changed_lines: Vec::new(),
            saved_by: None,
            search: Search::default(),
            queue: VecDeque::new(),
            in_flight: false,
            reading: None,
            pending: None,
            held: VecDeque::new(),
            replaying: false,
            overflowed: false,
            dropped_keys: 0,
            deferred: Vec::new(),
            reveal: false,
            view: (0, 0),
        }
    }

    /// A new connection: nothing that was on its way arrived, and the engine's file is read again.
    pub fn reconnect(&mut self, doc: FileDocument) -> Vec<Effect> {
        self.queue.clear();
        self.in_flight = false;
        self.reading = None;
        self.pending = None;
        self.replaying = false;
        self.abandon();
        // What waited for edits that are lost is not done: closing the pane the person is
        // editing, by a write that comes later, least of all.
        self.deferred.clear();
        self.lines = doc.line_count;
        self.doc = doc;
        self.reload()
    }

    /// The range an edit replaces. A caret that is not on the page moves to the first line in
    /// view, as the Mac's text view puts it in the page it shows.
    pub fn range_to_edit(&mut self) -> Option<TextRange> {
        let page = self.page.as_ref()?;
        let range = self.selection.range();
        if page.holds(range.start.line) && page.holds(range.end.line) {
            return Some(range);
        }
        let at = page.clamp(&position(self.view.0, 0));
        self.selection = Selection::caret(at.clone());
        Some(TextRange {
            start: at.clone(),
            end: at,
        })
    }

    /// Whether edits are on their way to the engine.
    pub fn has_unsent(&self) -> bool {
        self.in_flight || !self.queue.is_empty()
    }

    /// The caret has moved and the view should show it; true once.
    pub fn take_reveal(&mut self) -> bool {
        std::mem::take(&mut self.reveal)
    }

    /// The line break of the line the selection starts on: `\r\n` or `\n`.
    pub fn line_ending(&self) -> &'static str {
        self.page.as_ref().map_or(super::page::LF, |page| {
            page.ending(self.selection.range().start.line)
        })
    }

    pub fn selected_text(&self) -> Option<String> {
        let page = self.page.as_ref()?;
        let range = self.selection.range();
        // The selection is the one the jump left, not the one it is going to.
        (!self.selection.is_empty() && !self.is_landing())
            .then(|| page.between(&range.start, &range.end))
    }

    fn request(&self, from_line: u32) -> PageRequest {
        PageRequest {
            file_id: self.doc.file_id.clone(),
            from_line,
            lines: PAGE_LINES,
            attempt: 0,
        }
    }

    fn read(&mut self, from_line: u32) -> Vec<Effect> {
        if self.reading == Some(from_line) {
            return Vec::new();
        }
        self.reading = Some(from_line);
        vec![send(EditorCommand::Read(self.request(from_line)))]
    }

    /// Reads the first page again for a pane that holds none, as its Retry asks: the read that
    /// failed is over, so a new one is not the same read.
    pub fn read_first_page(&mut self) -> Vec<Effect> {
        self.reading = None;
        self.read(0)
    }

    /// The first line of the page to read: the one that holds the caret that is on its way, else
    /// the one loaded.
    fn start(&self) -> u32 {
        let loaded = self.page.as_ref();
        match (&self.pending, loaded) {
            (Some((start, _)), _) => *start,
            (None, Some(page)) => page.from_line,
            (None, None) => 0,
        }
    }

    /// Reads the page again, once the edits have reached the engine.
    pub fn reload(&mut self) -> Vec<Effect> {
        if self.has_unsent() {
            if !self.deferred.contains(&Deferred::Reload) {
                self.deferred.push(Deferred::Reload);
            }
            return Vec::new();
        }
        let start = self.start();
        self.reading = None;
        self.read(start)
    }

    /// Takes a page the engine sent. A page that arrives while edits are on their way is older
    /// than the page and is dropped; it is read again when they have arrived.
    pub fn install(&mut self, result: &FileReadResult, asked: u32) -> Vec<Effect> {
        // Only the page being waited for ends the wait: another one that arrives first leaves the
        // read that is outstanding to be waited for, and to fail.
        if self.reading == Some(result.from_line) {
            self.reading = None;
        }
        // A page read before the latest edit describes an older file.
        if self.has_unsent() || !at_least(&result.file.revision, &self.doc.revision) {
            return self.reload();
        }
        self.doc = result.file.clone();
        self.lines = self.doc.line_count;
        let mut page = Page::from_read(result);
        page.asked = asked;
        let keep = |at: &TextPosition| {
            if page.holds(at.line) {
                page.clamp(at)
            } else {
                at.clone()
            }
        };
        // The caret that is on its way goes with the page it waits for, not with an older one.
        let waited = match self.pending.take() {
            Some((start, target)) if start == result.from_line => Some(target),
            other => {
                self.pending = other;
                None
            }
        };
        let waited_line = waited.as_ref().map(|target| target.head.line);
        let waited_target = waited.clone();
        self.selection = match waited {
            Some(target) => {
                self.reveal = true;
                Selection::selecting(page.clamp(&target.anchor), page.clamp(&target.head))
            }
            None => Selection::selecting(keep(&self.selection.anchor), keep(&self.selection.head)),
        };
        // A page the engine cut short may end before the line the caret is going to: the page
        // is read again from that line, once.
        let missed = waited_line
            .filter(|line| *line >= page.to_line() && page.to_line() < self.lines)
            .filter(|line| *line != result.from_line);
        self.page = Some(page);
        let mut effects = Vec::new();
        if let (Some(line), Some(target)) = (missed, waited_target) {
            self.pending = Some((line, target));
            effects.extend(self.read(line));
        }
        effects.extend(self.release());
        effects.extend(self.search_again());
        effects
    }

    /// Opens the file from the answer to `file.open`, with the caret where it was.
    pub fn opened(&mut self, doc: &FileDocument, viewport: &FileReadResult) -> Vec<Effect> {
        self.doc = doc.clone();
        // The engine's first page of a file is a short one by choice.
        self.install(
            viewport,
            u32::try_from(viewport.lines.len()).unwrap_or(u32::MAX),
        )
    }

    /// Asks for the page the view needs when the page loaded does not hold it with room to spare.
    pub fn show(&mut self, first_line: u32, rows: u32) -> Vec<Effect> {
        self.view = (first_line, rows);
        let Some(page) = self.page.as_ref() else {
            return if self.has_unsent() {
                Vec::new()
            } else {
                self.read(0)
            };
        };
        let window = Window {
            visible_from: first_line,
            visible_lines: rows,
            loaded_from: page.from_line,
            loaded_to: page.to_line(),
            asked_lines: page.asked,
            total_lines: self.lines,
        };
        // One page at a time: the next view is judged when this page has arrived.
        match window.requested_start() {
            Some(start) if !self.has_unsent() && self.reading.is_none() => self.read(start),
            _ => Vec::new(),
        }
    }

    /// Remembers where the caret goes when the page that holds it has been read, and says where
    /// that page starts.
    fn wait_for(&mut self, target: Selection) -> u32 {
        let (from, to, asked) = self.page.as_ref().map_or((0, 0, 0), |page| {
            (page.from_line, page.to_line(), page.asked)
        });
        let start = landing_start(target.head.line, from, to, asked);
        self.pending = Some((start, target));
        start
    }

    /// Puts the selection where it is asked to be, reading the page that holds it when the page
    /// loaded does not.
    pub fn place(&mut self, target: Selection) -> Vec<Effect> {
        let Some(page) = self.page.as_ref() else {
            return Vec::new();
        };
        if page.holds(target.head.line) {
            self.selection = clamped(page, &target);
            self.reveal = true;
            return Vec::new();
        }
        let start = self.wait_for(target);
        self.read(start)
    }

    /// Moves the caret, or grows the selection when `extend`.
    pub fn motion(&mut self, motion: Motion, extend: bool) -> Vec<Effect> {
        let Some(page) = self.page.as_ref() else {
            return Vec::new();
        };
        let moved = target(page, self.lines, &self.selection, motion, extend);
        self.place(moved)
    }

    /// Selects the page: the part of the document that is loaded.
    pub fn select_all(&mut self) -> Vec<Effect> {
        let Some(page) = self.page.as_ref() else {
            return Vec::new();
        };
        let all = Selection::selecting(page.first(), page.last());
        self.place(all)
    }

    /// Makes what a keystroke changed: in the page now, and in the engine soon.
    pub fn edit(&mut self, plan: &Plan) -> Vec<Effect> {
        let Some(page) = self.page.as_mut() else {
            return Vec::new();
        };
        if !plan.apply(page) {
            return Vec::new();
        }
        let delta: i64 = plan
            .replacements
            .iter()
            .map(|made| added_lines(&made.text, made.range.end.line - made.range.start.line))
            .sum();
        self.lines = u32::try_from((i64::from(self.lines) + delta).max(1)).unwrap_or(u32::MAX);
        if let Some(caret) = plan.carets.last() {
            self.selection = Selection::caret(caret.clone());
        }
        self.doc.dirty = true;
        self.reveal = true;
        if !plan.edits.is_empty() {
            self.queue.push_back(plan.edits.clone());
        }
        self.flush()
    }

    /// Sends the next write, to the revision the one before made.
    fn flush(&mut self) -> Vec<Effect> {
        if self.in_flight {
            return Vec::new();
        }
        let Some(edits) = self.queue.pop_front() else {
            return Vec::new();
        };
        self.in_flight = true;
        vec![send(EditorCommand::Write {
            file_id: self.doc.file_id.clone(),
            base: self.doc.revision.clone(),
            edits,
        })]
    }

    /// The engine took a write.
    pub fn written(&mut self, result: FileWriteResult) -> Vec<Effect> {
        self.in_flight = false;
        self.doc = result.file;
        self.changed_lines = result.changed_lines;
        if let Some(page) = self.page.as_mut() {
            page.revision.clone_from(&self.doc.revision);
        }
        let mut effects = self.flush();
        if effects.is_empty() {
            self.lines = self.doc.line_count;
            effects = self.settled();
            // The matches of a search moved with the edits.
            effects.extend(self.search_again());
        }
        effects
    }

    /// The engine refused an edit, or never answered: what the replica made is not the file, so
    /// the page is read again.
    pub fn edit_failed(&mut self) -> Vec<Effect> {
        self.queue.clear();
        self.in_flight = false;
        self.pending = None;
        self.replaying = false;
        self.abandon();
        self.deferred.retain(|step| *step != Deferred::Reload);
        let mut effects = self.settled();
        effects.extend(self.reload());
        effects
    }

    /// A read did not come. A file that got shorter than the page's first line refuses it, and
    /// the view would stay on a page that is gone: the page is read again from the end of the
    /// file, or from its start. Of two reads that fail, the second is the last.
    /// `failed` is the first line of the page that was read.
    pub fn read_failed(&mut self, failed: u32) -> Vec<Effect> {
        let ours = self.reading == Some(failed);
        if ours {
            self.reading = None;
        }
        // The caret never lands when the page it waits for fails, or when no read is left that
        // could bring it: what waited for it has no place to go.
        let stranded = self
            .pending
            .as_ref()
            .is_some_and(|(start, _)| *start == failed || self.reading.is_none());
        let mut effects = Vec::new();
        if stranded {
            self.pending = None;
            self.abandon();
            if !self.has_unsent() {
                effects.extend(self.settled());
            }
        }
        if !ours || failed == 0 || self.has_unsent() {
            return effects;
        }
        let total = self.doc.line_count;
        let retry = if failed >= total {
            landing_start(total.saturating_sub(1), 0, 0, 0)
        } else {
            0
        };
        effects.extend(self.read(retry));
        effects
    }

    /// What was asked while edits were on their way is done now.
    fn settled(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        for step in std::mem::take(&mut self.deferred) {
            effects.extend(match step {
                Deferred::Reload => self.reload(),
                Deferred::Save => self.save(),
                Deferred::Replay { redo } => self.replay(redo),
                Deferred::Close => vec![self.close_effect()],
            });
        }
        effects
    }

    fn defer(&mut self, step: Deferred) {
        if !self.deferred.contains(&step) {
            self.deferred.push(step);
        }
    }

    fn close_effect(&self) -> Effect {
        Effect::Send(Command::ClosePane {
            pane_id: self.doc.pane_id.clone(),
        })
    }

    /// Closes the file's pane once its edits have reached the engine, which saves the draft.
    pub fn close(&mut self) -> Vec<Effect> {
        // Keys that wait for a jump are the person's too: they are typed before the pane closes.
        if self.has_unsent() || !self.held.is_empty() {
            self.defer(Deferred::Close);
            return Vec::new();
        }
        vec![self.close_effect()]
    }

    /// Waits for the engine to save the file, which it does shortly after the last edit.
    pub fn save(&mut self) -> Vec<Effect> {
        if self.has_unsent() {
            self.defer(Deferred::Save);
            return Vec::new();
        }
        let from = self.page.as_ref().map_or(0, |page| page.from_line);
        vec![send(EditorCommand::Save(self.request(from)))]
    }

    /// Undoes or redoes the newest edit the engine recorded; the history is the engine's, so
    /// it is shared by every client.
    pub fn replay(&mut self, redo: bool) -> Vec<Effect> {
        self.replaying = true;
        if self.has_unsent() {
            self.defer(Deferred::Replay { redo });
            return Vec::new();
        }
        vec![send(EditorCommand::Replay {
            file_id: self.doc.file_id.clone(),
            base: self.doc.revision.clone(),
            redo,
        })]
    }

    /// The engine undid or redid an edit: the page that shows the result is read, and the caret
    /// goes where the edit was.
    pub fn replayed(&mut self, result: FileUndoResult) -> Vec<Effect> {
        self.replaying = false;
        self.doc = result.file;
        self.changed_lines = result.changed_lines;
        if !result.applied {
            return self.release();
        }
        // What is done next is done on the page that shows the result, not on this one.
        let start = match result.cursor {
            Some(cursor) => self.wait_for(Selection::caret(cursor)),
            None => {
                let start = self.start();
                self.pending = Some((start, self.selection.clone()));
                start
            }
        };
        self.reading = None;
        self.read(start)
    }

    /// The engine saved the file, or somebody else changed it: a page read now has the highlights.
    pub fn changed_elsewhere(&mut self, doc: FileDocument) -> Vec<Effect> {
        // What the engine says of an older revision is not what this replica holds.
        if at_least(&doc.revision, &self.doc.revision) {
            if !self.has_unsent() {
                self.lines = doc.line_count;
            }
            self.doc = doc;
        }
        self.reload()
    }

    /// Searches the whole file from `from`. The answer takes the selection to the match when
    /// `navigate` says so.
    pub fn find(
        &mut self,
        query: FileFindQuery,
        from: TextPosition,
        forward: bool,
        navigate: bool,
    ) -> Vec<Effect> {
        let Some(page) = self.page.as_ref().filter(|_| !self.has_unsent()) else {
            return Vec::new();
        };
        self.search.seq += 1;
        self.search.navigate = navigate;
        self.search.again = Some((query.clone(), forward));
        vec![send(EditorCommand::Find {
            seq: self.search.seq,
            file_id: self.doc.file_id.clone(),
            revision: self.doc.revision.clone(),
            query,
            from,
            forward,
            page: PageRequest {
                file_id: self.doc.file_id.clone(),
                from_line: page.from_line,
                lines: page.len(),
                attempt: 0,
            },
        })]
    }

    /// What the search found. The match is selected, and when the page does not hold it the
    /// page that does is read and the search made again for the matches on it.
    pub fn found(&mut self, seq: u64, result: FileFindResult) -> Vec<Effect> {
        if seq != self.search.seq {
            return Vec::new();
        }
        let current = result.current.clone();
        self.search.result = Some(result);
        let Some(found) = current.filter(|_| self.search.navigate) else {
            return Vec::new();
        };
        self.search.navigate = false;
        self.place(Selection::selecting(found.range.start, found.range.end))
    }

    /// The search again, for the matches on the page just read, which may have changed.
    fn search_again(&mut self) -> Vec<Effect> {
        let Some((query, forward)) = self.search.again.clone() else {
            return Vec::new();
        };
        let from = self
            .search
            .result
            .as_ref()
            .and_then(|result| result.current.as_ref())
            .map_or_else(
                || self.selection.range().start,
                |found| found.range.start.clone(),
            );
        self.find(query, from, forward, false)
    }

    /// Forgets the search: the bar was closed.
    pub fn stop_search(&mut self) {
        self.search = Search {
            seq: self.search.seq + 1,
            ..Search::default()
        };
    }
}

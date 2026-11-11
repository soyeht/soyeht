//! Turns what the engine sends into records of what each terminal pane's screen shows.
//!
//! The recorder is a client like the window: it applies the engine's events to a `State`, which
//! keeps one `TerminalScreen` per terminal pane, and after each batch of events it looks at the
//! screens that changed. It never sends input (the only command it ever issues is the
//! subscription to a pane's output), and a record holds only text the screen shows, so keystrokes
//! are not in it: a program that does not echo (a password prompt) leaves nothing to record.

use crate::store::Store;
use serde_json::{Value, json};
use soyeht_client::{Command, Effect, Event, State, screen::TerminalScreen, streams::StreamOutput};
use soyeht_protocol::{Pane, PaneMode};
use std::collections::{BTreeMap, HashMap};

/// The format of the records, so that the analysis can tell a recording of another shape.
pub(crate) const FORMAT: u32 = 1;
/// A whole screen is recorded at most this often per pane.
pub(crate) const SCREEN_INTERVAL_MS: u64 = 1000;
/// How many rows above and below the cursor's row a change record carries.
const AROUND: u16 = 2;

/// When something happened: the wall clock, for correlating with other logs, and a monotonic
/// clock from the start of the run, for measuring gaps.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stamp {
    pub wall_ms: u64,
    pub mono_ms: u64,
}

/// What a connection's event asks of the program that runs the recorder.
#[derive(Debug, Default)]
pub(crate) struct Actions {
    pub subscribe: Vec<Command>,
    /// The replica can no longer be trusted: reconnect.
    pub resync: bool,
}

/// What a screen shows, as far as the records go.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    alt: bool,
    row: u16,
    column: u16,
    hidden: bool,
    columns: u16,
    rows: Vec<String>,
}

impl Seen {
    fn of(screen: &TerminalScreen) -> Self {
        let (height, columns) = screen.size();
        let cursor = screen.cursor_cell();
        Self {
            alt: screen.alternate_screen(),
            row: cursor.row,
            column: cursor.column,
            hidden: screen.cursor().is_none(),
            columns,
            rows: (0..height).map(|row| row_text(screen, row)).collect(),
        }
    }

    fn cursor(&self) -> Value {
        json!({"row": self.row, "col": self.column, "hidden": self.hidden})
    }

    fn text(&self, row: u16) -> &str {
        self.rows.get(usize::from(row)).map_or("", String::as_str)
    }
}

/// The text of a row with the blank end cut off. A character that was typed and a cell that was
/// never written cannot be told apart on a screen, so the cursor's column says where the end is.
fn row_text(screen: &TerminalScreen, row: u16) -> String {
    let text: String = screen
        .row_runs_within(row, u16::MAX)
        .iter()
        .map(|run| run.text.as_str())
        .collect();
    text.trim_end().to_owned()
}

struct Trace {
    header: Value,
    seq: u64,
    /// The file the last record went to; another one starts with the header and the screen.
    segment: Option<u64>,
    last: Seen,
    last_screen_ms: Option<u64>,
    screen_pending: bool,
}

#[derive(Default)]
struct Dirty {
    at: Option<Stamp>,
    bytes: usize,
    markers: Vec<&'static str>,
}

pub(crate) struct Recorder {
    state: State,
    store: Store,
    wanted: Vec<String>,
    traces: HashMap<String, Trace>,
    dirty: BTreeMap<String, Dirty>,
    notes: Vec<String>,
    write_failed: bool,
    said_full: bool,
}

impl Recorder {
    /// `wanted` names panes by id or by name; none means every terminal pane.
    pub fn new(store: Store, wanted: Vec<String>) -> Self {
        Self {
            state: State::default(),
            store,
            wanted,
            traces: HashMap::new(),
            dirty: BTreeMap::new(),
            notes: Vec::new(),
            write_failed: false,
            said_full: false,
        }
    }

    /// Lines for the person running the recorder, since the last call.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// Applies one event of the engine. The panes it changes are looked at by [`Self::end_batch`].
    pub fn handle(&mut self, event: Event, at: Stamp) -> Actions {
        match &event {
            Event::Terminal(output) if self.is_wanted(output.pane_id()) => {
                let dirty = self.dirty.entry(output.pane_id().to_owned()).or_default();
                dirty.at = Some(at);
                match output {
                    StreamOutput::Bytes { bytes, .. } => dirty.bytes += bytes.len(),
                    StreamOutput::Reset { .. } => dirty.markers.push("reset"),
                    StreamOutput::Gap { .. } => dirty.markers.push("gap"),
                }
            }
            Event::Connected(_) => self.mark_all("connected", at),
            Event::Disconnected(reason) => {
                self.notes.push(format!("disconnected: {reason}"));
                self.mark_all("disconnected", at);
            }
            Event::Problem { cause, .. } => self.notes.push(format!("engine problem: {cause}")),
            _ => {}
        }
        let mut actions = Actions::default();
        for effect in self.state.apply(event) {
            match effect {
                Effect::SubscribeTerminal(pane_id) if self.is_wanted(&pane_id) => {
                    actions.subscribe.push(Command::Subscribe { pane_id });
                }
                Effect::Resync => actions.resync = true,
                _ => {}
            }
        }
        actions
    }

    /// Looks at everything the events since the last call changed and records it.
    pub fn end_batch(&mut self, at: Stamp) {
        self.sync_panes(at);
        for (pane, dirty) in std::mem::take(&mut self.dirty) {
            let when = dirty.at.unwrap_or(at);
            if !self.traces.contains_key(&pane) {
                continue;
            }
            for what in dirty.markers {
                self.record(&pane, when, vec![marker(what)]);
            }
            self.observe(&pane, when, dirty.bytes);
        }
        if self.store.dropped() > 0 && !self.said_full {
            self.said_full = true;
            self.notes.push(
                "the files of this run reached their size cap: records are being dropped".into(),
            );
        }
    }

    /// Records the whole screen of the panes that changed within the last second, once the second
    /// since their last whole screen has passed.
    pub fn tick(&mut self, at: Stamp) {
        let due: Vec<String> = self
            .traces
            .iter()
            .filter(|(_, trace)| trace.screen_pending && screen_due(trace, at))
            .map(|(pane, _)| pane.clone())
            .collect();
        for pane in due {
            let Some(seen) = self.state.screen(&pane).map(Seen::of) else {
                continue;
            };
            let record = screen_record(&seen, "interval");
            self.record(&pane, at, vec![record]);
            self.screen_written(&pane, at);
        }
    }

    fn is_wanted(&self, pane_id: &str) -> bool {
        self.wanted.is_empty()
            || self.wanted.iter().any(|wanted| {
                wanted == pane_id
                    || self
                        .state
                        .pane(pane_id)
                        .is_some_and(|pane| pane.name == *wanted)
            })
    }

    fn mark_all(&mut self, what: &'static str, at: Stamp) {
        for pane in self.traces.keys() {
            let dirty = self.dirty.entry(pane.clone()).or_default();
            dirty.at = Some(at);
            dirty.markers.push(what);
        }
    }

    /// Starts a trace for each wanted terminal pane that has none, writes a new header when a
    /// pane's name or size changed, closes the trace of a pane that is gone, and fits each
    /// screen to the size the engine gives its pane (locally: the recorder never resizes a pane).
    fn sync_panes(&mut self, at: Stamp) {
        let panes: Vec<Pane> = self
            .state
            .panes()
            .iter()
            .filter(|pane| pane.mode == PaneMode::Terminal && self.is_wanted(&pane.pane_id))
            .cloned()
            .collect();
        for pane in &panes {
            let (rows, columns) = (
                u16::try_from(pane.rows).unwrap_or(u16::MAX),
                u16::try_from(pane.cols).unwrap_or(u16::MAX),
            );
            self.state.fit_terminal(&pane.pane_id, rows, columns);
            let header = header_of(pane);
            match self.traces.get_mut(&pane.pane_id) {
                None => self.begin(pane, header, at),
                Some(trace) if trace.header != header => {
                    trace.header = header.clone();
                    self.record(&pane.pane_id, at, vec![header]);
                }
                Some(_) => {}
            }
        }
        let gone: Vec<String> = self
            .traces
            .keys()
            .filter(|id| !panes.iter().any(|pane| pane.pane_id == **id))
            .cloned()
            .collect();
        for pane in gone {
            self.record(&pane, at, vec![marker("closed")]);
            self.traces.remove(&pane);
            self.store.close(&pane);
        }
    }

    fn begin(&mut self, pane: &Pane, header: Value, at: Stamp) {
        let Some(seen) = self.state.screen(&pane.pane_id).map(Seen::of) else {
            return;
        };
        self.notes
            .push(format!("recording pane {} ({})", pane.pane_id, pane.name));
        self.traces.insert(
            pane.pane_id.clone(),
            Trace {
                header,
                seq: 0,
                segment: None,
                last: seen,
                last_screen_ms: None,
                screen_pending: false,
            },
        );
        // Opens the file, which starts with the header and the screen.
        self.record(&pane.pane_id, at, Vec::new());
    }

    /// Records the change of `pane`'s screen, if what it shows changed.
    fn observe(&mut self, pane: &str, at: Stamp, bytes: usize) {
        let Some(seen) = self.state.screen(pane).map(Seen::of) else {
            return;
        };
        let Some(trace) = self.traces.get_mut(pane) else {
            return;
        };
        if trace.last == seen {
            return;
        }
        trace.seq += 1;
        let mut records = vec![change_record(&seen, trace.seq, bytes)];
        trace.last = seen.clone();
        let screen_now = screen_due(trace, at);
        trace.screen_pending = !screen_now;
        if screen_now {
            records.push(screen_record(&seen, "interval"));
        }
        self.record(pane, at, records);
        if screen_now {
            self.screen_written(pane, at);
        }
    }

    fn screen_written(&mut self, pane: &str, at: Stamp) {
        if let Some(trace) = self.traces.get_mut(pane) {
            trace.last_screen_ms = Some(at.mono_ms);
            trace.screen_pending = false;
        }
    }

    /// Writes `records` to the pane's file, preceded by its header and whole screen when the file
    /// is a new one, so that every file can be read alone. A write that fails is reported once and
    /// the recording goes on.
    fn record(&mut self, pane: &str, at: Stamp, records: Vec<Value>) {
        let mut lines: Vec<String> = records
            .into_iter()
            .map(|record| stamped(record, pane, at).to_string())
            .collect();
        let size: usize = lines.iter().map(|line| line.len() + 1).sum();
        let segment = match self.store.prepare(pane, at.wall_ms, size) {
            Ok(Some(segment)) => segment,
            Ok(None) => return,
            Err(error) => return self.failed(&error),
        };
        let Some(trace) = self.traces.get_mut(pane) else {
            return;
        };
        let mut intro = Vec::new();
        if trace.segment != Some(segment) {
            trace.segment = Some(segment);
            intro.push(stamped(trace.header.clone(), pane, at).to_string());
            if let Some(seen) = self.state.screen(pane).map(Seen::of) {
                intro.push(stamped(screen_record(&seen, "file"), pane, at).to_string());
                trace.last_screen_ms = Some(at.mono_ms);
                trace.screen_pending = false;
            }
        }
        intro.append(&mut lines);
        for line in intro {
            if let Err(error) = self.store.write(pane, &line) {
                return self.failed(&error);
            }
        }
    }

    fn failed(&mut self, error: &std::io::Error) {
        if !self.write_failed {
            self.write_failed = true;
            self.notes
                .push(format!("cannot write a recording: {error}"));
        }
    }
}

fn screen_due(trace: &Trace, at: Stamp) -> bool {
    trace
        .last_screen_ms
        .is_none_or(|last| at.mono_ms.saturating_sub(last) >= SCREEN_INTERVAL_MS)
}

/// What every record carries: the format, when, and which pane.
fn stamped(mut record: Value, pane: &str, at: Stamp) -> Value {
    if let Value::Object(fields) = &mut record {
        fields.insert("v".into(), json!(FORMAT));
        fields.insert("t".into(), json!(at.wall_ms));
        fields.insert("m".into(), json!(at.mono_ms));
        fields.insert("pane".into(), json!(pane));
    }
    record
}

fn marker(what: &str) -> Value {
    json!({"kind": "marker", "what": what})
}

/// The facts about a pane that the wire gives. The foreground program of the pty is not among
/// them: neither the engine nor the supervisor says it (`pid` is the process the pane started),
/// so the field is there and empty.
fn header_of(pane: &Pane) -> Value {
    json!({
        "kind": "pane",
        "name": pane.name,
        "cwd": pane.cwd,
        "cols": pane.cols,
        "rows": pane.rows,
        "pid": pane.pid,
        "agent": pane.agent,
        "foreground": null,
    })
}

fn change_record(seen: &Seen, seq: u64, bytes: usize) -> Value {
    let rows = u16::try_from(seen.rows.len()).unwrap_or(u16::MAX);
    let above: Vec<&str> = (seen.row.saturating_sub(AROUND)..seen.row)
        .map(|row| seen.text(row))
        .collect();
    let below: Vec<&str> = (seen.row + 1
        ..=seen.row.saturating_add(AROUND).min(rows.saturating_sub(1)))
        .map(|row| seen.text(row))
        .collect();
    json!({
        "kind": "change",
        "seq": seq,
        "alt": seen.alt,
        "cursor": seen.cursor(),
        "line": seen.text(seen.row),
        "above": above,
        "below": below,
        "outBytes": bytes,
    })
}

fn screen_record(seen: &Seen, why: &str) -> Value {
    json!({
        "kind": "screen",
        "why": why,
        "alt": seen.alt,
        "cursor": seen.cursor(),
        "cols": seen.columns,
        "rows": seen.rows,
    })
}

#[cfg(test)]
mod tests;

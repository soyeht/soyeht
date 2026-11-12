use super::*;
use crate::store::{
    Limits,
    tests::{DAY_ONE, Scratch},
};
use soyeht_client::Snapshot;
use soyeht_protocol::Workspace;
use std::{fs, path::PathBuf};

const ROWS: u32 = 8;
const COLUMNS: u32 = 40;

fn pane(id: &str, name: &str, mode: &str, rows: u32, columns: u32) -> Pane {
    serde_json::from_value(json!({
        "paneId": id, "workspaceId": "w", "name": name, "cwd": "/work", "mode": mode,
        "state": "idle", "source": "none", "confidence": 0.0, "phrase": "", "generation": 1,
        "cols": columns, "rows": rows, "record": false, "pid": 4242
    }))
    .unwrap()
}

fn snapshot(panes: Vec<Pane>) -> Snapshot {
    let workspace: Workspace = serde_json::from_value(
        json!({"workspaceId": "w", "name": "w", "path": "/work", "layout": {}}),
    )
    .unwrap();
    Snapshot {
        workspaces: vec![workspace],
        panes,
        ..Snapshot::default()
    }
}

fn limits() -> Limits {
    Limits {
        file_bytes: 1 << 20,
        keep: 3,
        total_bytes: 1 << 30,
    }
}

/// A recorder over a scratch folder and a clock the test moves.
struct Rig {
    scratch: Scratch,
    recorder: Recorder,
    now: u64,
}

impl Rig {
    fn with(limits: Limits, wanted: &[&str]) -> Self {
        let scratch = Scratch::new();
        let store = Store::new(scratch.join("screens"), limits).unwrap();
        let wanted = wanted.iter().map(ToString::to_string).collect();
        Self {
            scratch,
            recorder: Recorder::new(store, wanted),
            now: 0,
        }
    }

    fn new() -> Self {
        Self::with(limits(), &[])
    }

    fn at(&self) -> Stamp {
        Stamp {
            wall_ms: DAY_ONE + self.now,
            mono_ms: self.now,
        }
    }

    /// The engine lists `panes`; returns the panes the recorder subscribed to.
    fn connect(&mut self, panes: Vec<Pane>) -> Vec<String> {
        let at = self.at();
        let actions = self.recorder.handle(Event::Connected(snapshot(panes)), at);
        self.recorder.end_batch(at);
        actions
            .subscribe
            .into_iter()
            .map(|command| match command {
                Command::Subscribe { pane_id } => pane_id,
                other => panic!("the recorder sent {other:?}"),
            })
            .collect()
    }

    fn send(&mut self, id: &str, output: impl Fn(String) -> StreamOutput) {
        let at = self.at();
        self.recorder
            .handle(Event::Terminal(output(id.to_owned())), at);
    }

    fn end_batch(&mut self) {
        let at = self.at();
        self.recorder.end_batch(at);
        self.recorder.tick(at);
    }

    /// The engine sends `text` to `id`, `after_ms` after the last thing, and the recorder looks.
    fn feed(&mut self, id: &str, after_ms: u64, text: &str) {
        self.now += after_ms;
        let bytes = text.as_bytes().to_vec();
        self.send(id, |pane_id| StreamOutput::Bytes {
            pane_id,
            bytes: bytes.clone(),
        });
        self.end_batch();
    }

    fn reset(&mut self, id: &str) {
        self.send(id, |pane_id| StreamOutput::Reset { pane_id });
        self.end_batch();
    }

    /// The clock moves with nothing from the engine.
    fn wait(&mut self, ms: u64) {
        self.now += ms;
        self.end_batch();
    }

    fn directory(&self) -> PathBuf {
        let day = fs::read_dir(self.scratch.join("screens"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        day.path()
    }

    fn files(&self, id: &str) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = fs::read_dir(self.directory())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&format!("{id}.")))
            })
            .collect();
        found.sort();
        found
    }

    fn lines_of(path: &PathBuf) -> Vec<Value> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// Every record of the pane, the live file's.
    fn records(&self, id: &str) -> Vec<Value> {
        Self::lines_of(&self.directory().join(format!("{id}.jsonl")))
    }

    fn kind(&self, id: &str, kind: &str) -> Vec<Value> {
        self.records(id)
            .into_iter()
            .filter(|record| record["kind"] == kind)
            .collect()
    }
}

fn texts(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|text| text.as_str().unwrap())
        .collect()
}

fn terminal(id: &str) -> Pane {
    pane(id, id, "terminal", ROWS, COLUMNS)
}

#[test]
fn a_shell_prompt_with_typing_and_then_ctrl_u() {
    let mut rig = Rig::new();
    assert_eq!(rig.connect(vec![terminal("t1")]), ["t1"]);
    rig.reset("t1");
    rig.feed("t1", 40, "user@mac ~ % ");
    for typed in "git status".chars() {
        rig.feed("t1", 120, &typed.to_string());
    }
    // Ctrl-U in the line editor: back to the start of the typed text, then erase to the end.
    rig.feed("t1", 900, "\x1b[10D\x1b[K");
    rig.wait(1100);

    let changes = rig.kind("t1", "change");
    assert_eq!(changes.len(), 12);
    let lines: Vec<&str> = changes
        .iter()
        .map(|c| c["line"].as_str().unwrap())
        .collect();
    assert_eq!(lines[0], "user@mac ~ %");
    assert_eq!(lines[1], "user@mac ~ % g");
    assert_eq!(lines[10], "user@mac ~ % git status");
    assert_eq!(lines[11], "user@mac ~ %");
    let columns: Vec<u64> = changes
        .iter()
        .map(|c| c["cursor"]["col"].as_u64().unwrap())
        .collect();
    assert_eq!(columns[0], 13);
    assert_eq!(columns[10], 23);
    assert_eq!(columns[11], 13);
    let seqs: Vec<u64> = changes.iter().map(|c| c["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, (1..=12).collect::<Vec<u64>>());
    assert!(changes.iter().all(|c| c["alt"] == false));
    // The prompt is on the first row: nothing above, and blank rows below.
    assert_eq!(changes[10]["above"], json!([]));
    assert_eq!(texts(&changes[10]["below"]), ["", ""]);
    assert_eq!(changes[10]["outBytes"], 1);

    // The first record is the pane's header, the second the whole screen the file starts with.
    let records = rig.records("t1");
    assert_eq!(records[0]["kind"], "pane");
    assert_eq!(records[0]["foreground"], Value::Null);
    assert_eq!(records[0]["pid"], 4242);
    assert_eq!(records[1]["kind"], "screen");
    assert_eq!(records[1]["why"], "file");
    assert!(
        rig.kind("t1", "marker")
            .iter()
            .any(|m| m["what"] == "reset")
    );

    // A whole screen at most once a second; the last one shows the line erased.
    let screens = rig.kind("t1", "screen");
    let times: Vec<u64> = screens.iter().map(|s| s["m"].as_u64().unwrap()).collect();
    assert!(
        times
            .windows(2)
            .all(|pair| pair[1] - pair[0] >= SCREEN_INTERVAL_MS),
        "{times:?}"
    );
    let last = screens.last().unwrap();
    assert_eq!(last["rows"][0], "user@mac ~ %");
    assert_eq!(last["cursor"]["col"], 13);
    assert_eq!(last["rows"].as_array().unwrap().len(), ROWS as usize);
}

#[test]
fn a_tui_input_box_has_the_box_above_and_below_the_typed_line() {
    let mut rig = Rig::new();
    rig.connect(vec![terminal("t1")]);
    rig.reset("t1");
    let top = format!("╭{}╮", "─".repeat(20));
    let empty = format!("│ >{}│", " ".repeat(18));
    let bottom = format!("╰{}╯", "─".repeat(20));
    rig.feed(
        "t1",
        50,
        &format!("Welcome back\r\n\r\n{top}\r\n{empty}\r\n{bottom}\x1b[4;5H"),
    );
    for typed in "fix bug".chars() {
        rig.feed("t1", 150, &typed.to_string());
    }
    let typed = rig.kind("t1", "change").pop().unwrap();
    assert_eq!(typed["line"], format!("│ > fix bug{}│", " ".repeat(10)));
    assert_eq!(texts(&typed["above"]), ["", top.as_str()]);
    assert_eq!(texts(&typed["below"]), [bottom.as_str(), ""]);
    assert_eq!(
        typed["cursor"],
        json!({"row": 3, "col": 11, "hidden": false})
    );

    // Clearing the input box: the row is drawn empty again and the cursor goes back.
    rig.feed("t1", 2000, "\x1b[4;5H       \x1b[4;5H");
    let cleared = rig.kind("t1", "change").pop().unwrap();
    assert_eq!(cleared["line"], empty);
    assert_eq!(cleared["cursor"]["col"], 4);
    assert_eq!(cleared["alt"], false);
}

#[test]
fn an_editor_on_the_alternate_screen_and_the_shell_it_returns_to() {
    let mut rig = Rig::new();
    rig.connect(vec![terminal("t1")]);
    rig.reset("t1");
    rig.feed("t1", 10, "$ vim notes\r\n");
    rig.feed(
        "t1",
        300,
        "\x1b[?1049h\x1b[H\x1b[2Jline one\r\nline two\r\nline three\x1b[8;1H-- INSERT --\x1b[2;9H",
    );
    let inside = rig.kind("t1", "change").pop().unwrap();
    assert_eq!(inside["alt"], true);
    assert_eq!(inside["line"], "line two");
    assert_eq!(
        inside["cursor"],
        json!({"row": 1, "col": 8, "hidden": false})
    );
    assert_eq!(texts(&inside["above"]), ["line one"]);
    assert_eq!(texts(&inside["below"]), ["line three", ""]);

    // `dd` in a modal editor deletes the line: the line below moves up and the cursor stays.
    rig.feed("t1", 400, "\x1b[2;1H\x1b[M");
    let deleted = rig.kind("t1", "change").pop().unwrap();
    assert_eq!(deleted["line"], "line three");
    assert_eq!(deleted["alt"], true);

    rig.feed("t1", 400, "\x1b[?1049l");
    let back = rig.kind("t1", "change").pop().unwrap();
    assert_eq!(back["alt"], false);
    assert_eq!(back["line"], "");
    assert_eq!(back["cursor"], json!({"row": 1, "col": 0, "hidden": false}));
    assert_eq!(texts(&back["above"]), ["$ vim notes"]);
}

#[test]
fn a_chunk_that_changes_nothing_on_the_screen_records_nothing() {
    let mut rig = Rig::new();
    rig.connect(vec![terminal("t1")]);
    rig.reset("t1");
    rig.feed("t1", 10, "Password: ");
    // The person types with echo off: the program sends nothing that shows. Only a mode change.
    rig.feed("t1", 500, "\x1b[?2004h");
    rig.feed("t1", 500, "");
    assert_eq!(rig.kind("t1", "change").len(), 1);
}

#[test]
fn chunks_that_arrive_together_are_one_record() {
    let mut rig = Rig::new();
    rig.connect(vec![terminal("t1")]);
    rig.reset("t1");
    rig.now = 10;
    for chunk in ["one\r\n", "two\r\n", "three"] {
        rig.send("t1", |pane_id| StreamOutput::Bytes {
            pane_id,
            bytes: chunk.as_bytes().to_vec(),
        });
    }
    rig.end_batch();
    let changes = rig.kind("t1", "change");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["outBytes"], 15);
    assert_eq!(changes[0]["line"], "three");
    assert_eq!(texts(&changes[0]["above"]), ["one", "two"]);
}

#[test]
fn only_terminal_panes_the_run_asked_for_are_subscribed_to_and_recorded() {
    let mut rig = Rig::with(limits(), &["build", "t3"]);
    let subscribed = rig.connect(vec![
        pane("t1", "shell", "terminal", ROWS, COLUMNS),
        pane("t2", "build", "terminal", ROWS, COLUMNS),
        pane("t3", "other", "terminal", ROWS, COLUMNS),
        pane("a1", "build", "agent", ROWS, COLUMNS),
    ]);
    assert_eq!(subscribed, ["t2", "t3"]);
    for id in ["t1", "t2"] {
        rig.reset(id);
        rig.feed(id, 10, "hello");
    }
    let names: Vec<String> = fs::read_dir(rig.directory())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"t2.jsonl".to_owned()));
    assert!(names.contains(&"t3.jsonl".to_owned()));
    assert!(!names.contains(&"t1.jsonl".to_owned()));
    assert!(!names.contains(&"a1.jsonl".to_owned()));
}

#[test]
fn records_hold_only_what_the_screen_shows_and_never_input() {
    let allowed = |kind: &str| -> &[&str] {
        match kind {
            "pane" => &["name", "cwd", "cols", "rows", "pid", "agent", "foreground"],
            "screen" => &["why", "alt", "cursor", "cols", "rows"],
            "change" => &["seq", "alt", "cursor", "line", "above", "below", "outBytes"],
            "marker" => &["what"],
            other => panic!("a record of kind {other}"),
        }
    };
    let mut rig = Rig::new();
    rig.connect(vec![terminal("t1")]);
    rig.reset("t1");
    rig.feed("t1", 10, "$ ls\r\nfile\r\n$ ");
    rig.feed("t1", 1500, "x");
    rig.wait(1500);
    for record in rig.records("t1") {
        let kind = record["kind"].as_str().unwrap();
        let common = ["v", "t", "m", "pane", "kind"];
        for key in record.as_object().unwrap().keys() {
            assert!(
                common.contains(&key.as_str()) || allowed(kind).contains(&key.as_str()),
                "{kind} has {key}"
            );
        }
        assert_eq!(record["v"], FORMAT);
        assert_eq!(record["pane"], "t1");
        assert_eq!(record["t"], DAY_ONE + record["m"].as_u64().unwrap());
    }
}

#[test]
fn a_file_that_reaches_its_cap_is_rotated_and_each_file_starts_with_the_pane_and_the_screen() {
    let cap = 4000;
    let mut rig = Rig::with(
        Limits {
            file_bytes: cap,
            keep: 2,
            total_bytes: 1 << 30,
        },
        &[],
    );
    rig.connect(vec![terminal("t1")]);
    rig.reset("t1");
    for n in 0..300 {
        rig.feed("t1", 7, &format!("line number {n}\r\n"));
    }
    let files = rig.files("t1");
    assert_eq!(files.len(), 3, "{files:?}");
    for path in &files {
        let size = fs::metadata(path).unwrap().len();
        // A file may pass its cap by the header and screen that start it.
        assert!(size <= cap + 1500, "{path:?} holds {size} bytes");
        let records = Rig::lines_of(path);
        assert_eq!(records[0]["kind"], "pane");
        assert_eq!(records[1]["kind"], "screen");
        assert_eq!(records[1]["why"], "file");
    }
    // The newest file holds the end of the output.
    let live = rig.kind("t1", "change").pop().unwrap();
    assert_eq!(live["above"][1], "line number 299");
}

#[test]
fn a_run_that_reaches_its_total_cap_says_so_and_stops_growing() {
    let total = 3000;
    let mut rig = Rig::with(
        Limits {
            file_bytes: 1 << 20,
            keep: 3,
            total_bytes: total,
        },
        &[],
    );
    rig.connect(vec![terminal("t1")]);
    rig.reset("t1");
    for n in 0..100 {
        rig.feed("t1", 7, &format!("line number {n}\r\n"));
    }
    let size = fs::metadata(rig.directory().join("t1.jsonl"))
        .unwrap()
        .len();
    assert!(size <= total, "{size}");
    let notes = rig.recorder.take_notes();
    assert!(
        notes.iter().any(|note| note.contains("size cap")),
        "{notes:?}"
    );
}

#[test]
fn a_pane_the_engine_resizes_or_closes_is_followed_and_nothing_is_sent_back() {
    let mut rig = Rig::new();
    rig.connect(vec![terminal("t1"), terminal("t2")]);
    rig.reset("t1");
    rig.feed("t1", 10, "before");
    rig.now += 2000;
    // The same engine, again: pane t1 is larger, pane t2 is gone.
    let at = rig.at();
    let actions = rig.recorder.handle(
        Event::Connected(snapshot(vec![pane("t1", "t1", "terminal", 10, 50)])),
        at,
    );
    assert!(
        actions
            .subscribe
            .iter()
            .all(|c| matches!(c, Command::Subscribe { pane_id } if pane_id == "t1"))
    );
    assert!(!actions.resync);
    rig.end_batch();
    rig.feed("t1", 10, "!");
    rig.wait(1100);

    let headers = rig.kind("t1", "pane");
    assert_eq!(headers.last().unwrap()["cols"], 50);
    assert_eq!(headers.last().unwrap()["rows"], 10);
    let markers: Vec<_> = rig
        .kind("t1", "marker")
        .iter()
        .map(|m| m["what"].clone())
        .collect();
    assert!(markers.contains(&json!("connected")), "{markers:?}");
    let screen = rig.kind("t1", "screen").pop().unwrap();
    assert_eq!(screen["cols"], 50);
    assert_eq!(screen["rows"].as_array().unwrap().len(), 10);
    assert_eq!(screen["rows"][0], "before!");
    let gone = rig.kind("t2", "marker");
    assert_eq!(gone.last().unwrap()["what"], "closed");
}

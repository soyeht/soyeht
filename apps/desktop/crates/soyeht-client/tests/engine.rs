//! The whole client against a scripted engine on a real local endpoint: a Unix socket, or on
//! Windows the loopback endpoint file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_client::{
    CLIENT_NAME, Command, Connection, Effect, Engine, Error, Event, State,
    attachments::{Draft, Source},
    autostart::Autostart,
    editor::selection::Motion,
};
use soyeht_platform::local::{Listener, Stream};
use soyeht_protocol::{
    ErrorKind, EventName, PromptAttachmentKind, error_code,
    frame::{Frame, read_json, write_json},
};
use std::{path::PathBuf, time::Duration};

/// The engine side of one connection.
struct FakeEngine(Stream);

impl FakeEngine {
    fn accept(listener: &Listener) -> Self {
        let stream = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut engine = Self(stream);
        engine.send(&json!({"jsonrpc": "2.0", "method": "auth.challenge",
            "params": {"nonce": "AAAA", "engineId": "e1"}}));
        engine
    }

    fn send(&mut self, value: &Value) {
        write_json(&mut self.0, value).unwrap();
    }

    fn request(&mut self) -> Value {
        read_json(&mut self.0)
            .unwrap()
            .expect("client closed early")
    }

    fn reply(&mut self, request: &Value, result: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
    }

    fn expect(&mut self, method: &str) -> Value {
        let request = self.request();
        assert_eq!(request["method"], method);
        request
    }

    /// hello, state.snapshot (one workspace, one terminal) and events.subscribe.
    fn handshake(&mut self) {
        let hello = self.expect("hello");
        assert_eq!(hello["params"]["client"], CLIENT_NAME);
        self.reply(
            &hello,
            json!({"engineVersion": "0", "protocolVersion": 1,
            "connectionId": "c1", "role": "human_local", "capabilities": [], "dbId": "db"}),
        );
        let snapshot = self.expect("state.snapshot");
        self.send(&notification(
            "state.snapshot_part",
            json!({
            "snapshotId": "s1", "index": 0, "of": 2, "section": "workspaces",
            "data": [{"workspaceId": "w1", "name": "api", "path": "/work", "layout": {}}]}),
        ));
        self.send(&notification(
            "state.snapshot_part",
            json!({
            "snapshotId": "s1", "index": 1, "of": 2, "section": "panes",
            "data": [{"paneId": "t1", "workspaceId": "w1", "name": "shell", "cwd": "/work",
                "mode": "terminal", "state": "idle", "source": "none", "confidence": 0.0,
                "phrase": "", "generation": 1, "cols": 80, "rows": 24, "record": false}]}),
        ));
        self.reply(
            &snapshot,
            json!({"snapshotId": "s1", "cursor": "7", "parts": 2,
            "focus": {"workspaceId": "w1", "paneId": "t1"}}),
        );
        let subscribe = self.expect("events.subscribe");
        assert_eq!(subscribe["params"]["since"], "7");
        self.reply(&subscribe, json!({"subscriptionId": "sub", "through": "7"}));
    }
}

fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

/// The engine's refusal of `request` because what it names does not exist, with the code and the
/// kind the contract gives.
fn not_found(request: &Value, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": request["id"], "error": {
        "code": error_code(&ErrorKind::NotFound), "message": message,
        "data": {"kind": ErrorKind::NotFound}}})
}

fn socket_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("soyeht-client-{name}-{}.sock", std::process::id()))
}

fn next(inbox: &async_channel::Receiver<Event>) -> Event {
    inbox.recv_blocking().unwrap()
}

fn listen(name: &str) -> (Listener, PathBuf) {
    let path = socket_path(name);
    let _ = std::fs::remove_file(&path);
    (Listener::bind(&path).unwrap(), path)
}

/// An engine listening on a fresh socket, already connected and handshaken.
fn connected(name: &str) -> (Engine, async_channel::Receiver<Event>, FakeEngine, PathBuf) {
    let (listener, path) = listen(name);
    let (engine, inbox) = Engine::start(path.clone());
    let mut remote = FakeEngine::accept(&listener);
    remote.handshake();
    (engine, inbox, remote, path)
}

/// An engine that starts listening at `path` and answers the handshake of the client's connection.
fn bring_up(path: &std::path::Path) -> (Listener, FakeEngine) {
    let listener = Listener::bind(path).unwrap();
    let mut remote = FakeEngine::accept(&listener);
    remote.handshake();
    (listener, remote)
}

fn finish(engine: &Engine, remote: FakeEngine, path: &PathBuf) {
    engine.stop();
    drop(remote);
    let _ = std::fs::remove_file(path);
}

/// Subscribes terminal `t1`; the engine answers with stream `st1` on binary reference 5.
fn subscribe_t1(engine: &Engine, remote: &mut FakeEngine) -> Value {
    engine
        .send(Command::Subscribe {
            pane_id: "t1".into(),
        })
        .unwrap();
    let subscribe = remote.expect("pane.stream.subscribe");
    remote.reply(
        &subscribe,
        json!({"streamId": "st1", "paneId": "t1", "generation": 1,
        "firstRetainedOffset": "0", "nextOffset": "0", "binaryPaneRef": 5}),
    );
    subscribe
}

#[test]
fn connects_lists_state_streams_terminal_output_and_sends_input() {
    let (engine, inbox, mut remote, path) = connected("flow");
    let mut state = State::default();
    let effects = state.apply(next(&inbox));
    assert_eq!(state.connection, Connection::Connected);
    assert_eq!(state.workspaces()[0].name, "api");
    assert_eq!(state.focused_pane().unwrap().pane_id, "t1");
    assert!(effects.contains(&Effect::SubscribeTerminal("t1".into())));

    // The application subscribes the terminal; the engine replays, then streams live.
    let subscribe = subscribe_t1(&engine, &mut remote);
    assert_eq!(subscribe["params"]["from"], "tail");
    remote.send(&notification(
        "stream.replay_start",
        json!({
        "streamId": "st1", "generation": 1, "fromOffset": "0", "toOffset": "6", "reset": true}),
    ));
    Frame::Terminal {
        pane_ref: 5,
        generation: 1,
        offset: 0,
        bytes: b"$ ls\r\n".to_vec(),
    }
    .write_to(&mut remote.0)
    .unwrap();
    remote.send(&notification(
        "stream.replay_done",
        json!({"streamId": "st1", "nextOffset": "6"}),
    ));
    Frame::Terminal {
        pane_ref: 5,
        generation: 1,
        offset: 6,
        bytes: b"file\r\n".to_vec(),
    }
    .write_to(&mut remote.0)
    .unwrap();

    let mut screen_text = String::new();
    while !screen_text.contains("file") {
        state.apply(next(&inbox));
        screen_text = state.screen("t1").unwrap().contents();
    }
    assert!(screen_text.starts_with("$ ls"));

    // Typing reaches the engine as base64 pane.input and is acknowledged in full.
    engine
        .send(Command::Input {
            pane_id: "t1".into(),
            bytes: b"echo hi\r".to_vec(),
        })
        .unwrap();
    let input = remote.expect("pane.input");
    assert_eq!(input["params"]["paneId"], "t1");
    assert_eq!(input["params"]["bytes"], "ZWNobyBoaQ0=");
    remote.reply(&input, json!({"acceptedBytes": 8}));

    // Pushed events reach the state.
    remote.send(&notification(
        "pane.renamed",
        json!({"eventId": "8", "at": "now", "pane": {
        "paneId": "t1", "workspaceId": "w1", "name": "build", "cwd": "/work", "mode": "terminal",
        "state": "idle", "source": "none", "confidence": 0.0, "phrase": "", "generation": 1,
        "cols": 80, "rows": 24, "record": false}}),
    ));
    state.apply(next(&inbox));
    assert_eq!(state.panes()[0].name, "build");

    finish(&engine, remote, &path);
}

#[test]
fn reconnects_after_the_engine_goes_away() {
    let (listener, path) = listen("reconnect");
    let (engine, inbox) = Engine::start(path.clone());

    let mut first = FakeEngine::accept(&listener);
    first.handshake();
    assert!(matches!(next(&inbox), Event::Connected(_)));
    drop(first);
    assert!(matches!(next(&inbox), Event::Disconnected(_)));

    let mut second = FakeEngine::accept(&listener);
    second.handshake();
    assert!(matches!(next(&inbox), Event::Connected(_)));

    engine.stop();
    drop(second);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_engine_that_is_not_running_is_reported_and_retried() {
    let path = socket_path("absent");
    let _ = std::fs::remove_file(&path);
    let (engine, inbox) = Engine::start(path.clone());
    assert!(matches!(next(&inbox), Event::Disconnected(_)));

    let (_listener, remote) = bring_up(&path);
    assert!(matches!(next(&inbox), Event::Connected(_)));

    finish(&engine, remote, &path);
}

/// The next event that satisfies `wanted`, within ten seconds.
fn wait_for(inbox: &async_channel::Receiver<Event>, wanted: impl Fn(&Event) -> bool) -> Event {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match inbox.try_recv() {
            Ok(event) if wanted(&event) => return event,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    panic!("the event did not come");
}

#[test]
fn an_engine_that_ends_after_a_connection_is_started_again() {
    let path = socket_path("rearm");
    let _ = std::fs::remove_file(&path);
    let nowhere = std::env::temp_dir().join(format!("soyeht-no-programs-{}", std::process::id()));
    let start = Autostart::in_directory(nowhere.clone(), Some(nowhere));
    let (engine, inbox) = Engine::start_with(path.clone(), start);
    // The first start fails, as there are no programs, and is not tried again at the retry.
    wait_for(&inbox, |event| matches!(event, Event::Starting));
    wait_for(&inbox, |event| matches!(event, Event::Problem(_)));

    let (listener, remote) = bring_up(&path);
    wait_for(&inbox, |event| matches!(event, Event::Connected(_)));

    // The engine goes away: a connection was made, so the next absence is started again.
    drop(remote);
    drop(listener);
    let _ = std::fs::remove_file(&path);
    wait_for(&inbox, |event| matches!(event, Event::Starting));
    engine.stop();
}

#[test]
fn a_refused_request_becomes_a_problem_not_a_crash() {
    let (engine, inbox, mut remote, path) = connected("refused");
    assert!(matches!(next(&inbox), Event::Connected(_)));

    engine
        .send(Command::Focus {
            pane_id: "gone".into(),
        })
        .unwrap();
    let focus = remote.expect("pane.focus");
    remote.send(&not_found(&focus, "Pane not found"));
    match next(&inbox) {
        Event::Problem(message) => assert_eq!(message, "Pane not found"),
        other => panic!("expected a problem, got {other:?}"),
    }

    finish(&engine, remote, &path);
}

#[test]
fn typing_is_sent_while_fetches_still_wait_for_their_answers() {
    let (engine, inbox, mut remote, path) = connected("priority");
    assert!(matches!(next(&inbox), Event::Connected(_)));

    // A connection starts several fetches; the engine is slow to answer all of them.
    for pane in ["a1", "a2", "a3"] {
        engine
            .send(Command::LoadTranscript {
                pane_id: pane.into(),
            })
            .unwrap();
    }
    engine.send(Command::LoadUsage).unwrap();
    engine
        .send(Command::Input {
            pane_id: "t1".into(),
            bytes: b"ls".to_vec(),
        })
        .unwrap();
    // The keystroke arrives although no fetch has been answered.
    let mut fetches = Vec::new();
    let input = loop {
        let request = remote.request();
        if request["method"] == "pane.input" {
            break request;
        }
        fetches.push(request);
    };
    assert!(fetches.len() <= 4);
    remote.reply(&input, json!({"acceptedBytes": 2}));
    finish(&engine, remote, &path);
}

#[test]
fn opening_a_pane_is_answered_with_its_ticket_and_a_failure_releases_it() {
    let (engine, inbox, mut remote, path) = connected("tickets");
    assert!(matches!(next(&inbox), Event::Connected(_)));

    engine
        .send(Command::CreateTerminal {
            workspace_id: "w1".into(),
            cwd: None,
            ticket: 5,
        })
        .unwrap();
    let create = remote.expect("pane.create");
    remote.reply(&create, json!({"paneId": "t9"}));
    assert!(matches!(
        next(&inbox),
        Event::Opened { ticket: 5, pane_id } if pane_id == "t9"
    ));

    engine
        .send(Command::CreateTerminal {
            workspace_id: "w1".into(),
            cwd: None,
            ticket: 6,
        })
        .unwrap();
    let refused = remote.expect("pane.create");
    remote.send(&not_found(&refused, "No such workspace"));
    assert!(matches!(next(&inbox), Event::OpenFailed { ticket: 6 }));
    assert!(matches!(next(&inbox), Event::Problem(_)));

    finish(&engine, remote, &path);
}

#[test]
fn an_interface_that_stops_draining_stalls_the_queues_instead_of_growing_memory() {
    let path = socket_path("unbounded");
    let (engine, inbox) = Engine::start(path.clone());
    // Nothing listens, so every command ends as a problem for the interface, which never reads.
    let mut refused = None;
    for _ in 0..100_000 {
        let command = Command::Focus {
            pane_id: "t1".into(),
        };
        if let Err(error) = engine.send(command) {
            refused = Some(error);
            break;
        }
    }
    assert!(matches!(refused, Some(Error::Busy)), "{refused:?}");
    assert!(inbox.len() <= 1024, "{} events queued", inbox.len());
    drop(inbox);
    engine.stop();
}

#[test]
fn a_closed_pane_releases_its_stream_position() {
    let (engine, inbox, mut remote, path) = connected("closed");
    assert!(matches!(next(&inbox), Event::Connected(_)));
    subscribe_t1(&engine, &mut remote);
    remote.send(&notification(
        "stream.replay_start",
        json!({"streamId": "st1", "generation": 1, "fromOffset": "0", "toOffset": "0",
        "reset": true}),
    ));
    assert!(matches!(next(&inbox), Event::Terminal(_)));

    remote.send(&notification(
        "pane.closed",
        json!({"eventId": "8", "at": "now", "paneId": "t1", "workspaceId": "w1"}),
    ));
    // Output for a stream nobody tracks any more is dropped: the next event is the sentinel.
    Frame::Terminal {
        pane_ref: 5,
        generation: 1,
        offset: 0,
        bytes: b"late".to_vec(),
    }
    .write_to(&mut remote.0)
    .unwrap();
    remote.send(&notification("usage.changed", json!({})));
    assert!(matches!(
        next(&inbox),
        Event::Notification {
            name: EventName::PaneClosed,
            ..
        }
    ));
    assert!(matches!(
        next(&inbox),
        Event::Notification {
            name: EventName::UsageChanged,
            ..
        }
    ));

    finish(&engine, remote, &path);
}

fn read_frame(remote: &mut FakeEngine) -> Frame {
    Frame::read_from(&mut remote.0)
        .unwrap()
        .expect("client closed early")
}

#[test]
fn a_prompt_with_attachments_sends_their_bytes_as_frames_before_the_request() {
    let (engine, _inbox, mut remote, path) = connected("attachments");
    let upload = Draft::new(
        PromptAttachmentKind::Image,
        "shot.png".into(),
        "image/png".into(),
        3,
        Source::Bytes(vec![1, 2, 3]),
        None,
    );
    let linked = Draft::new(
        PromptAttachmentKind::File,
        "report.pdf".into(),
        "application/pdf".into(),
        2_048,
        Source::HostFile("/work/report.pdf".into()),
        None,
    );
    engine
        .send(Command::Prompt {
            pane_id: "t1".into(),
            text: "look".into(),
            attachments: vec![upload.clone(), linked],
        })
        .unwrap();
    let Frame::Attachment { header, bytes } = read_frame(&mut remote) else {
        panic!("the bytes go first, as an attachment frame");
    };
    assert_eq!(bytes, [1, 2, 3]);
    let prompt = remote.expect("pane.prompt");
    let sent = prompt["params"]["attachments"].as_array().unwrap();
    assert_eq!(sent[0]["attachmentId"], upload.id);
    assert_eq!(
        header,
        *uuid::Uuid::parse_str(&upload.id).unwrap().as_bytes()
    );
    assert_eq!(sent[1]["path"], "/work/report.pdf");
    assert!(sent[1].get("attachmentId").is_none());
    remote.reply(&prompt, json!({}));
    finish(&engine, remote, &path);
}

#[test]
fn the_queue_of_waiting_messages_is_read_to_its_last_page() {
    let (engine, inbox, mut remote, path) = connected("queue");
    next(&inbox);
    engine
        .send(Command::LoadQueue {
            pane_id: "a1".into(),
        })
        .unwrap();
    let item = |id: &str| {
        json!({"promptId": id, "text": id, "origin": "human", "status": "queued",
               "queuedAt": "2026-10-11T12:00:00Z"})
    };
    let first = remote.expect("pane.queue.list");
    assert!(first["params"].get("cursor").is_none());
    remote.reply(
        &first,
        json!({"items": [item("q1")], "draft": false, "nextCursor": "c2"}),
    );
    let second = remote.expect("pane.queue.list");
    assert_eq!(second["params"]["cursor"], "c2");
    remote.reply(
        &second,
        json!({"items": [item("q2")], "draft": false, "paused": true}),
    );
    let Event::Queue {
        pane_id,
        items,
        paused,
    } = next(&inbox)
    else {
        panic!("the queue arrives as one event");
    };
    assert_eq!(pane_id, "a1");
    assert_eq!(
        items
            .iter()
            .map(|i| i.prompt_id.as_str())
            .collect::<Vec<_>>(),
        ["q1", "q2"]
    );
    assert!(paused);
    finish(&engine, remote, &path);
}

/// The document the engine lists for the file `main.rs` of pane `e1`.
fn document(revision: u32, dirty: bool) -> Value {
    json!({"fileId": "f1", "paneId": "e1", "workspaceId": "w1", "path": "main.rs",
        "language": "rust", "revision": revision.to_string(), "diskRevision": "1", "lineCount": 1,
        "byteCount": "2", "dirty": dirty, "conflicted": false, "watching": true})
}

/// A read of `main.rs` that says `text`.
fn read_of(revision: u32, dirty: bool, text: &str) -> Value {
    json!({"file": document(revision, dirty), "source": "buffer", "fromLine": 0, "toLine": 1,
        "lines": [{"line": 0, "text": text, "highlights": []}]})
}

/// Sends what the replica asks the engine for.
fn dispatch(engine: &Engine, effects: Vec<Effect>) {
    for effect in effects {
        if let Effect::Send(command) = effect {
            engine.send(command).unwrap();
        }
    }
}

fn page_text(state: &State) -> String {
    let file = state.editors.file_of_pane("e1").unwrap();
    file.page.as_ref().unwrap().text(0).to_owned()
}

#[test]
fn a_file_is_opened_edited_to_the_revision_the_engine_gave_and_saved() {
    let (engine, inbox, mut remote, path) = connected("editor");
    let mut state = State::default();
    state.apply(next(&inbox));

    dispatch(&engine, state.editor_open("w1", "main.rs", true));
    let open = remote.expect("file.open");
    assert_eq!(open["params"]["path"], "main.rs");
    assert_eq!(open["params"]["preview"], true);
    let pane = json!({"paneId": "e1", "workspaceId": "w1", "name": "main.rs", "cwd": "/work",
        "mode": "editor", "state": "idle", "source": "none", "confidence": 0.0, "phrase": "",
        "generation": 1, "cols": 80, "rows": 24, "record": false});
    remote.reply(
        &open,
        json!({"file": document(1, false), "pane": pane, "viewport": read_of(1, false, "hi")}),
    );
    let effects = state.apply(next(&inbox));
    assert!(effects.contains(&Effect::FocusPane("e1".into())));
    assert_eq!(page_text(&state), "hi");

    // A keystroke changes the page at once; the write names the revision and who writes.
    dispatch(&engine, state.editors.motion("e1", Motion::LineEnd, false));
    dispatch(&engine, state.editors.insert("e1", "!"));
    assert_eq!(page_text(&state), "hi!");
    let write = remote.expect("file.write");
    assert_eq!(write["params"]["baseRevision"], "1");
    assert_eq!(write["params"]["author"]["actor"]["role"], "human_local");
    assert_eq!(write["params"]["author"]["actor"]["client"], CLIENT_NAME);
    assert_eq!(write["params"]["edits"][0]["text"], "!");
    remote.reply(
        &write,
        json!({"file": document(2, true), "changedLines": [0]}),
    );
    dispatch(&engine, state.apply(next(&inbox)));

    // Saving waits until the engine has saved: the page is read until the file is not dirty.
    dispatch(&engine, state.editors.save("e1"));
    let first = remote.expect("file.read");
    remote.reply(&first, read_of(2, true, "hi!"));
    let second = remote.expect("file.read");
    remote.reply(&second, read_of(2, false, "hi!"));
    state.apply(next(&inbox));
    assert!(!state.editors.file_of_pane("e1").unwrap().doc.dirty);

    finish(&engine, remote, &path);
}

use crate::{core::Core, ports::StorePort, test_panes::core_with_agent};
use serde_json::{Value, json};
use soyeht_acp::rpc::{Message, Record};
use soyeht_protocol::{TranscriptKind, TranscriptMessage, ndjson::Decoder};
use std::path::PathBuf;

const PANE: &str = "agent";

fn line(update: Value) -> String {
    let notification = json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":update}});
    format!("{notification}\n")
}

fn text(kind: &str, text: &str) -> String {
    line(json!({"sessionUpdate":kind,"content":{"type":"text","text":text}}))
}

fn tool(update: &str, status: &str) -> String {
    let id = json!({"toolCallId":"t1","title":"Read","kind":"read","status":status,"locations":[]});
    let mut update_json = id;
    update_json["sessionUpdate"] = json!(update);
    line(update_json)
}

/// A request the engine answers with an error: a record that cannot wait in a batch.
fn request() -> String {
    format!(
        "{}\n",
        json!({"jsonrpc":"2.0","id":9,"method":"unknown/method","params":{}})
    )
}

fn core(loading: bool) -> (Core, PathBuf) {
    let (mut core, directory) = core_with_agent(&[]);
    let running = core.agents.get_mut(PANE).unwrap();
    running.state.session_id = Some("s1".into());
    if loading {
        running.state.phase = crate::agents::state::Phase::Loading;
    }
    (core, directory)
}

fn feed(core: &mut Core, lines: &[String]) {
    let screen = &mut core.panes.get_mut(PANE).unwrap().screen;
    let next = screen.next_offset();
    screen
        .append_data(next, lines.concat().into_bytes())
        .unwrap();
}

/// The same lines as records, each committed on its own, as before the poll batched them;
/// `start` is the offset of the first.
fn one_commit_per_record(core: &mut Core, lines: &[String], start: usize) {
    let mut at = start as u64;
    for line in lines {
        let message = Message::parse(line.trim_end().as_bytes()).unwrap();
        let record = Record {
            from_seq: at,
            to_seq: at + line.len() as u64,
            message,
        };
        at += line.len() as u64;
        core.native_record(PANE, record, true).unwrap();
        core.agents.get_mut(PANE).unwrap().decoder = Decoder::new(at);
    }
}

fn messages(core: &Core, count: u64) -> Vec<TranscriptMessage> {
    (0..count)
        .filter_map(|seq| core.store.agent_message(PANE, seq).unwrap())
        .map(|message| TranscriptMessage {
            // The engine names a message at random; the sequence says which one it is.
            local_id: format!("message-{}", message.seq),
            ..message
        })
        .collect()
}

fn updates(core: &mut Core) -> Vec<Value> {
    let (_, events) = core.store.replay(0).unwrap();
    events
        .into_iter()
        .filter(|event| event.method == "pane.update")
        .map(|event| event.params)
        .collect()
}

#[test]
fn a_poll_commits_once_and_sends_one_event_per_message_whatever_the_number_of_chunks() {
    let mut work = Vec::new();
    for chunks in [3, 300] {
        let (mut core, directory) = core(false);
        let lines: Vec<String> = (0..chunks)
            .map(|n| text("agent_message_chunk", &format!("{n:03}")))
            .collect();
        feed(&mut core, &lines);
        let before = soyeht_store::test_support::statements_started();
        core.decode_agent_log(PANE).unwrap();
        work.push(soyeht_store::test_support::statements_started() - before);

        let all = messages(&core, 4);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].text.len(), 3 * chunks);
        let chunk_events: Vec<Value> = updates(&mut core)
            .into_iter()
            .filter(|u| u["kind"] == "agent_message_chunk")
            .collect();
        assert_eq!(chunk_events.len(), 1, "one event for the message");
        assert_eq!(
            chunk_events[0]["text"].as_str().unwrap().len(),
            3 * chunks,
            "with the whole text"
        );
        let stored = core.store.agent_runtime(PANE).unwrap().unwrap();
        assert_eq!(
            stored.acknowledged_offset,
            core.panes[PANE].screen.next_offset()
        );
        crate::test_paths::remove_after(core, directory);
    }
    assert_eq!(
        work[0], work[1],
        "the work of a poll does not grow with its records"
    );
}

/// Delivers `lines` in `parts` pieces, one poll each, and returns what the pane holds.
fn batched(
    lines: &[String],
    loading: bool,
    parts: usize,
) -> (Vec<TranscriptMessage>, Value, Vec<Value>) {
    let (mut core, directory) = core(loading);
    for part in lines.chunks(lines.len().div_ceil(parts)) {
        feed(&mut core, part);
        core.decode_agent_log(PANE).unwrap();
    }
    let result = observe(&mut core, lines.len() as u64);
    crate::test_paths::remove_after(core, directory);
    result
}

fn observe(core: &mut Core, count: u64) -> (Vec<TranscriptMessage>, Value, Vec<Value>) {
    let mut state = serde_json::to_value(&core.agents.agent(PANE).state).unwrap();
    // The capabilities of a state are random.
    for name in ["binding", "hook_binding"] {
        state.as_object_mut().map(|fields| fields.remove(name));
    }
    (messages(core, count), state, updates(core))
}

/// The events a commit per record sent, with each earlier state of a message left out, as a
/// batch does: the last event of a message is where it was.
fn without_superseded(events: Vec<Value>) -> Vec<Value> {
    let whole = |u: &Value| u["kind"] != "terminal_output" && u["kind"] != "config_option_update";
    (0..events.len())
        .filter(|at| {
            !whole(&events[*at])
                || !events[at + 1..].iter().any(|later| {
                    later["kind"] == events[*at]["kind"]
                        && later["localId"] == events[*at]["localId"]
                })
        })
        .map(|at| events[at].clone())
        .collect()
}

/// The events without the random name of their message.
fn unnamed(mut events: Vec<Value>) -> Vec<Value> {
    for event in &mut events {
        for field in ["localId", "eventId", "at"] {
            event.as_object_mut().map(|fields| fields.remove(field));
        }
    }
    events
}

fn stream() -> Vec<String> {
    vec![
        text("agent_thought_chunk", "Look"),
        text("agent_thought_chunk", "ing"),
        text("agent_message_chunk", "I will "),
        text("agent_message_chunk", "read it."),
        tool("tool_call", "in_progress"),
        tool("tool_call_update", "in_progress"),
        request(),
        tool("tool_call_update", "completed"),
        text("agent_message_chunk", "Done"),
        text("agent_message_chunk", "."),
    ]
}

#[test]
fn polls_of_any_size_leave_what_a_commit_per_record_left() {
    for loading in [false, true] {
        let lines = stream();
        let (mut core, directory) = core(loading);
        // The record that cannot wait ends a batch, so the events of the stream are those of
        // the records before it and those after it, each group with its superseded ones left out.
        let mut events = Vec::new();
        let (mut sent, mut start, mut seen) = (0, 0, 0);
        for end in [6, 7, lines.len()] {
            one_commit_per_record(&mut core, &lines[sent..end], start);
            start += lines[sent..end].iter().map(String::len).sum::<usize>();
            sent = end;
            let all = updates(&mut core);
            events.extend(without_superseded(all[seen..].to_vec()));
            seen = all.len();
        }
        let (messages, state, _) = observe(&mut core, 20);
        crate::test_paths::remove_after(core, directory);
        assert!(messages.iter().any(|m| m.kind == TranscriptKind::Tool) || loading);

        for parts in [1, 3, 10] {
            let (batched_messages, batched_state, batched_events) = batched(&lines, loading, parts);
            assert_eq!(
                batched_messages, messages,
                "messages, {parts} polls, loading {loading}"
            );
            assert_eq!(
                batched_state, state,
                "state, {parts} polls, loading {loading}"
            );
            if parts == 1 {
                assert_eq!(
                    unnamed(batched_events),
                    unnamed(events.clone()),
                    "events, loading {loading}"
                );
            }
        }
    }
}

#[test]
fn a_record_that_fails_leaves_the_records_before_it_committed_and_the_decoder_after_them() {
    let (mut core, directory) = core(false);
    let mut lines = vec![
        text("agent_message_chunk", "kept"),
        text("agent_message_chunk", " too"),
    ];
    lines.push("this is not json\n".into());
    // The bad line is in a record of its own: a record that does not decode applies none of it.
    feed(&mut core, &lines[..2]);
    feed(&mut core, &lines[2..]);
    assert!(core.decode_agent_log(PANE).is_err());
    assert_eq!(messages(&core, 4)[0].text, "kept too");
    let kept = (lines[0].len() + lines[1].len()) as u64;
    assert_eq!(core.agents.agent(PANE).state.acknowledged_offset, kept);
    assert_eq!(core.agents.agent(PANE).decoder.next_offset(), kept);
    crate::test_paths::remove_after(core, directory);
}

//! What the client knows about an agent pane: its transcript, the request waiting for an
//! answer, and the composer's control row built from the harness's own options.
//!
//! Every update the engine pushes carries the whole message it changes, so folding an update in
//! replaces the message with the same `localId`.

use crate::{Error, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_protocol::{
    AgentMessageUpdateKind, ConfigOption, ConfigValue, Pane, PaneState, PaneUpdateEvent,
    PermissionRequest, PlanStatus, TerminalOutput, ToolCall, ToolStatus, TranscriptKind,
    TranscriptMessage, TranscriptRole,
};
use std::time::{Duration, Instant};

/// The transcript page the client asks for, as on the Mac client.
pub const TRANSCRIPT_PAGE: u32 = 100;
/// The messages of a transcript the view draws; earlier ones stay in the replica.
pub const TAIL: usize = 200;
/// The tool calls the header ticker lists after the current activity.
const TICKER_HISTORY: usize = 3;
/// What stands between two entries of the header ticker.
pub const TICKER_SEPARATOR: &str = "   \u{2022}   ";

/// Whether an agent in `state` is in the middle of a turn: its composer offers stop and queues
/// what is typed, and its header counts the time.
pub fn is_running(state: &PaneState) -> bool {
    matches!(
        state,
        PaneState::Working | PaneState::WaitingForYou | PaneState::Cancelling | PaneState::NoSignal
    )
}

/// When the running turn started, as this device saw it: the engine does not report a turn's
/// start, so the clock notes the moment the active prompt first appeared.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnClock {
    prompt: Option<String>,
    started: Option<Instant>,
}

impl TurnClock {
    /// The pane says `prompt` is the one being answered (none when it is not answering one).
    pub fn observe(&mut self, prompt: Option<&str>, now: Instant) {
        if prompt == self.prompt.as_deref() {
            return;
        }
        self.prompt = prompt.map(str::to_owned);
        self.started = prompt.map(|_| now);
    }

    pub fn elapsed(&self, now: Instant) -> Option<Duration> {
        self.started
            .map(|started| now.saturating_duration_since(started))
    }
}

/// A turn's length as the header shows it: "2m 5s", or "1h 3m" from an hour on.
pub fn elapsed_text(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let (hours, minutes) = (seconds / 3_600, seconds % 3_600 / 60);
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m {}s", seconds % 60)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentView {
    pub messages: Vec<TranscriptMessage>,
    /// How long the running turn has been going.
    pub turn: TurnClock,
    /// The request the agent is waiting on, if any.
    pub permission: Option<PermissionRequest>,
    /// Why the session is not usable, when the engine said so.
    pub notice: Option<String>,
}

impl AgentView {
    /// Folds a message in: a new one is inserted at its place, a known one is replaced unless
    /// what is held is further along (a reply fetched earlier must not undo a newer update).
    pub fn merge(&mut self, message: TranscriptMessage) -> Result<()> {
        let order = sequence(&message)?;
        if let Some(known) = self
            .messages
            .iter_mut()
            .find(|known| known.local_id == message.local_id)
        {
            if supersedes(&message, known) {
                *known = message;
            }
            return Ok(());
        }
        let at = self.messages.partition_point(|held| {
            sequence(held)
                .is_ok_and(|held_order| (held_order, &held.local_id) < (order, &message.local_id))
        });
        self.messages.insert(at, message);
        Ok(())
    }

    /// Folds a fetched page in. Nothing already known is dropped: updates that arrived while the
    /// page was on its way stay.
    pub fn merge_page(&mut self, page: Vec<TranscriptMessage>) -> Result<()> {
        page.into_iter().try_for_each(|message| self.merge(message))
    }

    /// The messages to draw: the last `TAIL`, and how many earlier ones are left out.
    pub fn tail(&self) -> (&[TranscriptMessage], usize) {
        let hidden = self.messages.len().saturating_sub(TAIL);
        (&self.messages[hidden..], hidden)
    }

    /// What the header's ticker says, newest first and one entry after the other: the plan step in
    /// progress with the latest tool call, the tool calls before it, then the session's title.
    pub fn ticker(&self, title: Option<&str>) -> Option<String> {
        let tools: Vec<&ToolCall> = self.tools().collect();
        let mut items = Vec::new();
        let mut now = Vec::new();
        if let Some(plan) = self
            .messages
            .iter()
            .rev()
            .find(|message| message.kind == TranscriptKind::Plan)
            .and_then(|message| message.plan.as_ref())
        {
            let step = plan
                .iter()
                .position(|entry| entry.status == PlanStatus::InProgress)
                .or_else(|| {
                    plan.iter()
                        .position(|entry| entry.status == PlanStatus::Pending)
                });
            if let Some(step) = step {
                now.push(format!(
                    "Step {}/{}: {}",
                    step + 1,
                    plan.len(),
                    plan[step].content
                ));
            }
        }
        now.extend(tools.first().map(|tool| describe(tool)));
        let current = !now.is_empty();
        if current {
            items.push(now.join(" \u{b7} "));
        }
        items.extend(
            tools
                .iter()
                .skip(usize::from(current))
                .take(TICKER_HISTORY)
                .map(|tool| describe(tool)),
        );
        let title = title.map(str::trim).filter(|title| !title.is_empty());
        items.extend(title.map(str::to_owned));
        (!items.is_empty()).then(|| items.join(TICKER_SEPARATOR))
    }

    /// The latest tool calls, newest first, for callers that list more than the current one.
    pub fn tools(&self) -> impl Iterator<Item = &ToolCall> {
        self.messages
            .iter()
            .rev()
            .filter_map(|message| message.tool.as_ref())
            .take(1 + TICKER_HISTORY)
    }

    /// The share of the context window the last usage report says is in use.
    pub fn context_fraction(&self) -> Option<f64> {
        let usage = self
            .messages
            .iter()
            .rev()
            .find_map(|message| message.usage.as_ref())?;
        let (Ok(used), Ok(size)) = (
            usage.total_tokens.parse::<f64>(),
            usage.context_size.as_ref()?.parse::<f64>(),
        ) else {
            return None;
        };
        (size > 0.0).then(|| (used / size).clamp(0.0, 1.0))
    }
}

/// The text of a command's output, as the Mac shows it: the bytes the engine kept, read as UTF-8
/// (what is not valid becomes the replacement character). Output that is not base64 is empty.
pub fn terminal_text(output: &TerminalOutput) -> String {
    STANDARD
        .decode(&output.bytes)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

fn sequence(message: &TranscriptMessage) -> Result<u64> {
    message
        .seq
        .parse()
        .map_err(|_| Error::Invalid("a transcript message has a sequence that is not a decimal"))
}

/// How far a tool call has got.
fn progress(status: &ToolStatus) -> u8 {
    match status {
        ToolStatus::Pending => 0,
        ToolStatus::InProgress => 1,
        ToolStatus::Completed | ToolStatus::Failed => 2,
    }
}

/// Whether `incoming` is at least as far along as `known`: a finished message beats an unfinished
/// one, a longer text beats a shorter one, a tool call further on beats one behind.
fn supersedes(incoming: &TranscriptMessage, known: &TranscriptMessage) -> bool {
    if incoming.complete != known.complete {
        return incoming.complete;
    }
    if incoming.complete {
        return true;
    }
    match (&incoming.tool, &known.tool) {
        (Some(new), Some(old)) => progress(&new.status) >= progress(&old.status),
        _ => incoming.text.len() >= known.text.len(),
    }
}

fn describe(tool: &ToolCall) -> String {
    match tool.status {
        ToolStatus::Pending => format!("Waiting: {}", tool.title),
        ToolStatus::InProgress => format!("Running {}", tool.title),
        ToolStatus::Completed => format!("Done: {}", tool.title),
        ToolStatus::Failed => format!("Failed: {}", tool.title),
    }
}

/// A message with only the fields every kind has.
fn base(
    local_id: String,
    generation: u32,
    seq: String,
    (role, kind): (TranscriptRole, TranscriptKind),
    text: String,
    complete: bool,
) -> TranscriptMessage {
    TranscriptMessage {
        local_id,
        generation,
        seq,
        role,
        kind,
        text,
        complete,
        prompt_id: None,
        tool: None,
        terminal: None,
        plan: None,
        usage: None,
        agent_message: None,
        subagent: None,
        subagent_id: None,
        attachments: None,
    }
}

const ASSISTANT: TranscriptRole = TranscriptRole::Assistant;

/// The pane an update belongs to and the message it leaves behind. Terminal output,
/// configuration changes and subagent runs arrive with the transcript instead.
pub fn message_of(update: PaneUpdateEvent) -> Option<(String, TranscriptMessage)> {
    Some(match update {
        PaneUpdateEvent::V0(value) => {
            let kind = if value.kind == AgentMessageUpdateKind::SystemMessage {
                (TranscriptRole::System, TranscriptKind::System)
            } else {
                (ASSISTANT, TranscriptKind::Message)
            };
            let mut built = base(
                value.local_id,
                value.generation,
                value.seq,
                kind,
                value.text,
                value.complete,
            );
            built.subagent_id = value.subagent_id;
            (value.pane_id, built)
        }
        PaneUpdateEvent::V1(value) => {
            let mut built = base(
                value.local_id,
                value.generation,
                value.seq,
                (ASSISTANT, TranscriptKind::Thought),
                value.text,
                value.complete,
            );
            built.subagent_id = value.subagent_id;
            (value.pane_id, built)
        }
        PaneUpdateEvent::V2(value) => {
            let complete = value.tool.status == ToolStatus::Completed;
            tool_entry(
                value.pane_id,
                value.local_id,
                value.generation,
                value.seq,
                value.tool,
                value.subagent_id,
                complete,
            )
        }
        PaneUpdateEvent::V3(value) => {
            let complete = matches!(
                value.tool.status,
                ToolStatus::Completed | ToolStatus::Failed
            );
            tool_entry(
                value.pane_id,
                value.local_id,
                value.generation,
                value.seq,
                value.tool,
                value.subagent_id,
                complete,
            )
        }
        PaneUpdateEvent::V4(value) => {
            let mut built = base(
                value.local_id,
                value.generation,
                value.seq,
                (ASSISTANT, TranscriptKind::Plan),
                String::new(),
                true,
            );
            built.plan = Some(value.entries);
            built.subagent_id = value.subagent_id;
            (value.pane_id, built)
        }
        PaneUpdateEvent::V5(value) => {
            let mut built = base(
                value.local_id,
                value.generation,
                value.seq,
                (ASSISTANT, TranscriptKind::Usage),
                String::new(),
                true,
            );
            built.usage = Some(value.usage);
            (value.pane_id, built)
        }
        PaneUpdateEvent::V6(_) | PaneUpdateEvent::V7(_) | PaneUpdateEvent::V8(_) => return None,
    })
}

/// A tool call as a transcript message, with the pane it belongs to.
fn tool_entry(
    pane_id: String,
    local_id: String,
    generation: u32,
    seq: String,
    tool: ToolCall,
    subagent_id: Option<String>,
    complete: bool,
) -> (String, TranscriptMessage) {
    let mut built = base(
        local_id,
        generation,
        seq,
        (ASSISTANT, TranscriptKind::Tool),
        String::new(),
        complete,
    );
    built.tool = Some(tool);
    built.subagent_id = subagent_id;
    (pane_id, built)
}

/// One of the harness's own selectable options as a composer menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub config_id: String,
    pub title: String,
    pub choices: Vec<(String, String)>,
    pub current: Option<String>,
}

impl Menu {
    fn new(option: &ConfigOption) -> Option<Self> {
        if option.options.is_empty() {
            return None;
        }
        let ConfigValue::V0(current) = &option.current_value else {
            return None;
        };
        Some(Self {
            config_id: option.id.clone(),
            title: option.name.clone(),
            choices: option
                .options
                .iter()
                .map(|choice| (choice.value.clone(), choice.name.clone()))
                .collect(),
            current: Some(current.clone()),
        })
    }

    /// The name on the chip: the current choice, else the option's own title.
    pub fn label(&self) -> &str {
        self.current
            .as_ref()
            .and_then(|current| {
                self.choices
                    .iter()
                    .find(|(value, _)| value == current)
                    .map(|(_, name)| name.as_str())
            })
            .unwrap_or(&self.title)
    }
}

/// What the send button does right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Send { enabled: bool },
    Stop { enabled: bool },
}

/// The composer's control row, built from the pane's session: harness, model, effort,
/// permission mode, the context ring, and the send button.
#[derive(Debug, Clone, PartialEq)]
pub struct Controls {
    pub harness: String,
    pub model: Option<Menu>,
    pub effort: Option<Menu>,
    pub permission: Option<Menu>,
    pub placeholder: String,
    pub action: Action,
    pub context: Option<f64>,
    /// A change of configuration is being applied; the menus wait for it.
    pub busy: bool,
}

fn category(option: &ConfigOption) -> &str {
    option.category.as_deref().unwrap_or(&option.id)
}

impl Controls {
    /// `harness_name` is the display name of the pane's harness, when the catalogue has it.
    pub fn new(
        pane: &Pane,
        context: Option<f64>,
        harness_name: Option<&str>,
        has_draft: bool,
        connected: bool,
    ) -> Option<Self> {
        let agent = pane.agent.as_ref()?;
        let find = |wanted: &dyn Fn(&ConfigOption) -> bool| {
            agent.options.iter().find(|option| wanted(option))
        };
        let running = is_running(&pane.state);
        let harness = harness_name.unwrap_or(&agent.harness).to_owned();
        Some(Self {
            model: find(&|option| category(option) == "model").and_then(Menu::new),
            effort: find(&|option| category(option) == "thought_level").and_then(Menu::new),
            permission: find(&|option| {
                option.category.as_deref() == Some("mode") || option.id == "mode"
            })
            .and_then(Menu::new),
            placeholder: if running {
                format!("Queue a message for {harness}\u{2026}")
            } else {
                format!("Message {harness}\u{2026}")
            },
            action: if running {
                Action::Stop {
                    enabled: pane.state != PaneState::Cancelling && connected,
                }
            } else {
                Action::Send {
                    enabled: has_draft && connected,
                }
            },
            context,
            busy: agent.operation.is_some(),
            harness,
        })
    }

    /// The percentage the context ring's label shows.
    pub fn context_label(&self) -> String {
        self.context.map_or_else(
            || "\u{2013}".to_owned(),
            |fraction| format!("{}%", (fraction * 100.0).round()),
        )
    }
}

/// The status word at the header's trailing edge.
pub fn status_word(state: &PaneState, needs_authentication: bool, has_session: bool) -> String {
    if needs_authentication {
        return "Sign in".into();
    }
    if *state == PaneState::Starting && has_session {
        return "ready".into();
    }
    state_name(state).replace('_', " ")
}

/// The wire spelling of a pane state ("waiting_for_you").
pub fn state_name(state: &PaneState) -> &'static str {
    match state {
        PaneState::Starting => "starting",
        PaneState::Working => "working",
        PaneState::Idle => "idle",
        PaneState::WaitingForYou => "waiting_for_you",
        PaneState::Cancelling => "cancelling",
        PaneState::NoSignal => "no_signal",
        PaneState::Recovering => "recovering",
        PaneState::Dead => "dead",
        PaneState::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::pane_json;
    use serde_json::{Value, json};

    fn message_json(id: &str, seq: &str, kind: &str, text: &str) -> Value {
        json!({"localId": id, "generation": 1, "seq": seq, "role": "assistant", "kind": kind,
               "text": text, "complete": true})
    }

    fn message(id: &str, seq: &str, text: &str) -> TranscriptMessage {
        serde_json::from_value(message_json(id, seq, "message", text)).unwrap()
    }

    #[test]
    fn merging_replaces_by_local_id_and_orders_by_sequence() {
        let mut view = AgentView::default();
        view.merge(message("b", "2", "second")).unwrap();
        view.merge(message("a", "1", "first")).unwrap();
        view.merge(message("b", "2", "second, grown")).unwrap();
        let texts: Vec<_> = view.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["first", "second, grown"]);
    }

    fn partial(id: &str, seq: &str, text: &str, complete: bool) -> TranscriptMessage {
        let mut value = message(id, seq, text);
        value.complete = complete;
        value
    }

    #[test]
    fn a_page_fetched_earlier_never_undoes_a_newer_update() {
        let mut view = AgentView::default();
        view.merge(partial("m", "2", "Hello, wor", false)).unwrap();
        view.merge(partial("m", "2", "Hello, world", true)).unwrap();
        // The page was read before the last chunks: it is behind what is held.
        view.merge_page(vec![
            partial("m", "2", "Hello", false),
            message("a", "1", "first"),
        ])
        .unwrap();
        let held: Vec<_> = view
            .messages
            .iter()
            .map(|m| (m.text.as_str(), m.complete))
            .collect();
        assert_eq!(held, [("first", true), ("Hello, world", true)]);
    }

    #[test]
    fn a_page_adds_messages_without_dropping_the_known_ones() {
        let mut view = AgentView::default();
        view.merge(message("live", "9", "newer")).unwrap();
        view.merge_page(vec![message("old", "1", "older")]).unwrap();
        assert_eq!(view.messages.len(), 2);
        assert_eq!(view.messages[0].local_id, "old");
    }

    #[test]
    fn a_sequence_that_is_not_a_decimal_is_refused() {
        let mut view = AgentView::default();
        assert!(view.merge(message("x", "not a number", "text")).is_err());
        assert!(view.messages.is_empty());
    }

    #[test]
    fn the_tail_leaves_out_the_oldest_messages() {
        let mut view = AgentView::default();
        for index in 0..(TAIL + 5) {
            view.merge(message(&format!("m{index}"), &index.to_string(), "t"))
                .unwrap();
        }
        let (shown, hidden) = view.tail();
        assert_eq!((shown.len(), hidden), (TAIL, 5));
        assert_eq!(shown[0].local_id, "m5");
    }

    #[test]
    fn a_message_update_becomes_a_transcript_message() {
        let update: PaneUpdateEvent = serde_json::from_value(json!({
            "eventId": "9", "at": "now", "paneId": "p", "kind": "agent_message_chunk",
            "generation": 1, "seq": "3", "localId": "m", "text": "Hello", "complete": false
        }))
        .unwrap();
        let (pane_id, message) = message_of(update).unwrap();
        assert_eq!(pane_id, "p");
        assert_eq!(message.role, TranscriptRole::Assistant);
        assert_eq!(message.kind, TranscriptKind::Message);
        assert!(!message.complete);
    }

    #[test]
    fn a_tool_update_carries_the_call_and_completes_with_it() {
        let update: PaneUpdateEvent = serde_json::from_value(json!({
            "eventId": "9", "at": "now", "paneId": "p", "kind": "tool_call",
            "generation": 1, "seq": "4", "localId": "t",
            "tool": {"toolCallId": "c", "title": "ls", "kind": "execute",
                     "status": "completed", "locations": []}
        }))
        .unwrap();
        let (_, message) = message_of(update).unwrap();
        assert_eq!(message.kind, TranscriptKind::Tool);
        assert!(message.complete);
        let mut view = AgentView::default();
        view.merge(message).unwrap();
        assert_eq!(view.ticker(None).as_deref(), Some("Done: ls"));
    }

    #[test]
    fn a_commands_output_is_its_bytes_read_as_text() {
        let output = |bytes: &str| -> TerminalOutput {
            serde_json::from_value(json!({
                "terminalId": "t", "bytes": bytes, "offset": "0", "truncated": false
            }))
            .unwrap()
        };
        assert_eq!(terminal_text(&output("aGkKb2s=")), "hi\nok");
        assert_eq!(terminal_text(&output("/w==")), "\u{fffd}");
        assert_eq!(terminal_text(&output("not base64!")), "");
    }

    #[test]
    fn the_ticker_lists_the_current_step_then_earlier_tool_calls_then_the_title() {
        let tool = |seq: &str, title: &str, status: &str| {
            let mut value = message_json(seq, seq, "tool", "");
            value["tool"] = json!({"toolCallId": seq, "title": title, "status": status,
                                   "locations": []});
            serde_json::from_value::<TranscriptMessage>(value).unwrap()
        };
        let mut view = AgentView::default();
        assert_eq!(view.ticker(None), None);
        assert_eq!(view.ticker(Some("  ")), None);
        for (seq, title, status) in [
            ("1", "read a", "completed"),
            ("2", "edit b", "completed"),
            ("3", "test c", "in_progress"),
        ] {
            view.merge(tool(seq, title, status)).unwrap();
        }
        let mut plan = message_json("p", "4", "plan", "");
        plan["plan"] = json!([
            {"content": "write", "priority": "high", "status": "completed"},
            {"content": "check", "priority": "high", "status": "in_progress"}
        ]);
        view.merge(serde_json::from_value(plan).unwrap()).unwrap();
        let line = view.ticker(Some("The title")).unwrap();
        assert_eq!(
            line,
            [
                "Step 2/2: check \u{b7} Running test c",
                "Done: edit b",
                "Done: read a",
                "The title"
            ]
            .join(TICKER_SEPARATOR)
        );
    }

    #[test]
    fn a_turn_is_timed_from_the_moment_its_prompt_appears() {
        let start = Instant::now();
        let mut clock = TurnClock::default();
        assert_eq!(clock.elapsed(start), None);
        clock.observe(Some("p1"), start);
        clock.observe(Some("p1"), start + Duration::from_secs(5));
        assert_eq!(
            clock.elapsed(start + Duration::from_secs(65)),
            Some(Duration::from_secs(65))
        );
        clock.observe(Some("p2"), start + Duration::from_secs(70));
        assert_eq!(
            clock.elapsed(start + Duration::from_secs(71)),
            Some(Duration::from_secs(1))
        );
        clock.observe(None, start + Duration::from_secs(80));
        assert_eq!(clock.elapsed(start + Duration::from_secs(90)), None);
        assert_eq!(elapsed_text(Duration::from_secs(125)), "2m 5s");
        assert_eq!(elapsed_text(Duration::from_secs(3_780)), "1h 3m");
    }

    #[test]
    fn the_context_ring_follows_the_last_usage_report() {
        let mut view = AgentView::default();
        assert_eq!(view.context_fraction(), None);
        let mut usage = message("u", "5", "");
        usage.kind = TranscriptKind::Usage;
        usage.usage =
            serde_json::from_value(json!({"totalTokens": "250", "contextSize": "1000"})).unwrap();
        view.merge(usage).unwrap();
        assert_eq!(view.context_fraction(), Some(0.25));
    }

    fn agent_pane(state: &str) -> Pane {
        let mut value = pane_json("p", "w", "agent");
        value["state"] = json!(state);
        value["agent"] = json!({
            "harness": "claude", "version": "1", "fingerprint": "f", "authMethods": [],
            "queue": [], "draft": false, "role": "worker",
            "options": [
                {"id": "model", "name": "Model", "type": "select", "currentValue": "opus",
                 "options": [{"value": "opus", "name": "Opus"}, {"value": "sonnet", "name": "Sonnet"}]},
                {"id": "mode", "name": "Mode", "type": "select", "currentValue": "manual",
                 "options": [{"value": "manual", "name": "Manual"}]}
            ]
        });
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn the_control_row_comes_from_the_options_the_harness_advertises() {
        let pane = agent_pane("idle");
        let controls = Controls::new(&pane, None, Some("Claude"), false, true).unwrap();
        assert_eq!(controls.model.as_ref().map(Menu::label), Some("Opus"));
        assert_eq!(
            controls.permission.as_ref().map(Menu::label),
            Some("Manual")
        );
        assert!(controls.effort.is_none());
        assert_eq!(controls.placeholder, "Message Claude\u{2026}");
        assert_eq!(controls.action, Action::Send { enabled: false });
        assert_eq!(controls.context_label(), "\u{2013}");
    }

    #[test]
    fn a_running_agent_turns_send_into_stop_and_queues_messages() {
        let pane = agent_pane("working");
        let controls = Controls::new(&pane, None, None, true, true).unwrap();
        assert_eq!(controls.action, Action::Stop { enabled: true });
        assert_eq!(controls.placeholder, "Queue a message for claude\u{2026}");
        let cancelling = agent_pane("cancelling");
        let controls = Controls::new(&cancelling, None, None, true, true).unwrap();
        assert_eq!(controls.action, Action::Stop { enabled: false });
    }

    #[test]
    fn a_pane_without_a_session_has_no_control_row() {
        let pane: Pane = serde_json::from_value(pane_json("p", "w", "agent")).unwrap();
        assert!(Controls::new(&pane, None, None, true, true).is_none());
    }

    #[test]
    fn the_status_word_names_the_state() {
        assert_eq!(status_word(&PaneState::Idle, false, true), "idle");
        assert_eq!(
            status_word(&PaneState::WaitingForYou, false, true),
            "waiting for you"
        );
        assert_eq!(status_word(&PaneState::Starting, false, true), "ready");
        assert_eq!(status_word(&PaneState::Idle, true, true), "Sign in");
    }
}

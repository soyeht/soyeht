//! What the client knows about an agent pane: its transcript, the request waiting for an
//! answer, and the composer's control row built from the harness's own options.
//!
//! Every update the engine pushes carries the whole message it changes, so folding an update in
//! replaces the message with the same `localId`.

use crate::{Error, Result, cards::CardState, switch::HarnessSwitchView};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_protocol::{
    AgentMessageUpdateKind, ConfigOption, ConfigValue, Pane, PaneState, PaneUpdateEvent,
    PermissionRequest, PlanStatus, TerminalOutput, ToolCall, ToolStatus, TranscriptKind,
    TranscriptMessage, TranscriptRole, client_policy::CAPS_TICKER_HISTORY,
};
use std::time::{Duration, Instant};

/// The messages of a transcript the view draws; earlier ones stay in the replica.
pub const TAIL: usize = 200;
/// What stands between two entries of the header ticker.
pub const TICKER_SEPARATOR: &str = "   \u{2022}   ";

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
    /// The queue, the question, the sign-in and the paging behind the cards.
    pub cards: CardState,
    /// The pane's last switch to another harness.
    pub harness_switch: Option<HarnessSwitchView>,
}

impl AgentView {
    /// Folds an update in: a new message is inserted at its place and a known one is replaced,
    /// as the update carries the whole message.
    pub fn merge(&mut self, message: TranscriptMessage) -> Result<()> {
        self.fold(message, |_, _| true)
    }

    /// Folds a fetched page in. Nothing already known is dropped, and a known message that is
    /// further along stays: the page may have been read before the updates that arrived while it
    /// was on its way.
    pub fn merge_page(&mut self, page: Vec<TranscriptMessage>) -> Result<()> {
        page.into_iter()
            .try_for_each(|message| self.fold(message, supersedes))
    }

    /// Puts `message` at its place, replacing the known one with its `localId` when `replaces` says
    /// so.
    fn fold(
        &mut self,
        message: TranscriptMessage,
        replaces: fn(&TranscriptMessage, &TranscriptMessage) -> bool,
    ) -> Result<()> {
        let order = sequence(&message)?;
        if let Some(index) = self
            .messages
            .iter()
            .position(|known| known.local_id == message.local_id)
        {
            if !replaces(&message, &self.messages[index]) {
                return Ok(());
            }
            // The replacement goes where its sequence puts it, as on the Mac.
            self.messages.remove(index);
        }
        let at = self.messages.partition_point(|held| {
            sequence(held)
                .is_ok_and(|held_order| (held_order, &held.local_id) < (order, &message.local_id))
        });
        self.messages.insert(at, message);
        Ok(())
    }

    /// The messages to draw: the last `limit`, and how many earlier ones are left out.
    pub fn tail(&self, limit: usize) -> (&[TranscriptMessage], usize) {
        let hidden = self.messages.len().saturating_sub(limit);
        (&self.messages[hidden..], hidden)
    }

    /// What the header's ticker says, newest first and one entry after the other: the plan step in
    /// progress with the latest tool call, the tool calls before it, then the session's title.
    pub fn ticker(&self, title: Option<&str>) -> Option<String> {
        let tools: Vec<&ToolCall> = self.tools().collect();
        let mut items = Vec::new();
        let mut now = Vec::new();
        if let Some(plan) = self
            .latest_turn()
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
                .take(CAPS_TICKER_HISTORY)
                .map(|tool| describe(tool)),
        );
        let title = title.map(str::trim).filter(|title| !title.is_empty());
        items.extend(title.map(str::to_owned));
        (!items.is_empty()).then(|| items.join(TICKER_SEPARATOR))
    }

    /// The messages since the person's last prompt. An earlier turn's tool calls and plan say
    /// nothing about what the agent is doing now, so a new turn starts the ticker clean.
    fn latest_turn(&self) -> &[TranscriptMessage] {
        let start = self.messages.iter().rposition(|message| {
            message.role == TranscriptRole::User && message.kind == TranscriptKind::Message
        });
        &self.messages[start.map_or(0, |index| index + 1)..]
    }

    /// The latest turn's tool calls, newest first, for callers that list more than the current
    /// one. A failure that a later call of the same turn got past is left out: the retry
    /// succeeded, so the header no longer reports it.
    pub fn tools(&self) -> impl Iterator<Item = &ToolCall> {
        let mut recovered = false;
        self.latest_turn()
            .iter()
            .rev()
            .filter_map(|message| message.tool.as_ref())
            .filter(move |tool| {
                let keep = !(recovered && tool.status == ToolStatus::Failed);
                recovered |= tool.status == ToolStatus::Completed;
                keep
            })
            .take(1 + CAPS_TICKER_HISTORY)
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
/// one, a longer text beats a shorter one, a tool call further on beats one behind, and a command
/// output with more bytes beats one with fewer (its text is empty; the output is in `terminal`).
fn supersedes(incoming: &TranscriptMessage, known: &TranscriptMessage) -> bool {
    if incoming.complete != known.complete {
        return incoming.complete;
    }
    if incoming.complete {
        return true;
    }
    match (
        &incoming.tool,
        &known.tool,
        &incoming.terminal,
        &known.terminal,
    ) {
        (Some(new), Some(old), _, _) => progress(&new.status) >= progress(&old.status),
        (_, _, Some(new), Some(old)) => new.bytes.len() >= old.bytes.len(),
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
        let running = pane.state.is_turn_running();
        let harness = harness_name.unwrap_or(&agent.harness).to_owned();
        Some(Self {
            model: find(&ConfigOption::is_model).and_then(Menu::new),
            effort: find(&ConfigOption::is_effort).and_then(Menu::new),
            permission: find(&ConfigOption::is_permission_mode).and_then(Menu::new),
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
    state.as_str().replace('_', " ")
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vector_tests;

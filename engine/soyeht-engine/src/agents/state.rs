use crate::errors::{self, Result};
use serde::{Deserialize, Deserializer, Serialize};
use soyeht_acp::{Client, config::Options, session::Initialization};
use soyeht_protocol::{
    AgentCommand, AgentHarnessSwitch, AgentPaneRole, AgentSession, ConfigOperation, ConfigValue,
    HarnessLaunch, PaneQueueListResult, PromptAttachment, PromptItem, PromptOrigin, PromptStatus,
    QueuedPromptSummary, Role, TaskExecutor, limits,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use uuid::Uuid;

pub(crate) use super::continuation::InternalContinuation;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HarnessSwitchCheckpoint {
    pub(crate) user: super::pending::UserOperation,
    pub(crate) from_harness: String,
    pub(crate) to_harness: String,
    #[serde(default)]
    pub(crate) to_name: String,
    pub(crate) candidate_instance_key: String,
    pub(crate) previous_queue_paused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RetiredAgentProcess {
    pub(crate) instance_key: String,
    pub(crate) instance_id: String,
    pub(crate) supervisor_id: String,
}

/// A replayed user message held back until it is complete, so the engine's
/// continuation prompt is recognized however the adapter splits it.
#[derive(Debug, Clone)]
pub(crate) struct ReplayedUserMessage {
    pub(crate) message_id: Option<String>,
    pub(crate) text: String,
    /// Set once the message outgrew the prompt and streams as a user turn.
    pub(crate) streaming: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    Initializing,
    ProbeNew,
    ProbeList,
    ProbeLoad,
    ProbeConfig,
    RestartAfterProbe,
    Opening,
    Loading,
    Ready,
    NeedsLogin,
    Recovering,
    Faulted,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Binding(String);

impl std::fmt::Debug for Binding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Binding([redacted])")
    }
}

impl Binding {
    pub(crate) fn generate() -> Result<Self> {
        Ok(Self(crate::auth::token()?))
    }

    pub(crate) fn matches(&self, candidate: &str) -> bool {
        !self.0.is_empty()
            && self.0.len() == candidate.len()
            && crate::auth::same_token(&self.0, candidate)
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    pub(crate) fn sha256(&self) -> String {
        crate::auth::token_hash(&self.0)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub(crate) binding: Binding,
    #[serde(default)]
    pub(crate) hook_binding: Binding,
    #[serde(default = "default_mcp_role")]
    pub(crate) mcp_role: Role,
    #[serde(default = "default_true")]
    pub(crate) mcp_enabled: bool,
    pub launch: HarnessLaunch,
    pub binary_digest: String,
    pub fingerprint: String,
    pub client: Client,
    pub initialized: Option<Initialization>,
    pub phase: Phase,
    /// A prompt-level auth failure survives native session reopen until sign-in or a valid turn.
    #[serde(default)]
    pub auth_required: bool,
    /// The protocol sign-in method (`agent` type) the person signed in with. A new agent
    /// process is authenticated with it before it opens a session.
    #[serde(default)]
    pub(crate) agent_sign_in: Option<String>,
    pub session_id: Option<String>,
    pub options: Options,
    /// The slash commands the agent last advertised, served by `pane.commands.list`.
    #[serde(default)]
    pub commands: Vec<AgentCommand>,
    /// The permission mode this pane keeps: the one requested at creation,
    /// then the last one the harness confirmed. Re-applied after a restart or
    /// harness switch when the harness offers the same mode.
    #[serde(default)]
    pub mode: Option<String>,
    pub operation: Option<ConfigOperation>,
    #[serde(default)]
    pub operation_timed_out: bool,
    pub operation_user: Option<super::pending::UserOperation>,
    #[serde(deserialize_with = "requestable_options")]
    pub requested_options: BTreeMap<String, ConfigValue>,
    pub initial_option: Option<String>,
    pub initial_config_failed: bool,
    #[serde(skip)]
    pub queue: VecDeque<PromptItem>,
    pub queue_revision: u64,
    /// Queued prompts already written to the transcript because the agent could
    /// not start them (it waits for sign-in). Starting one does not record it again.
    #[serde(default)]
    pub(crate) transcribed_prompts: BTreeSet<String>,
    #[serde(default)]
    pub queue_paused: bool,
    pub active: Option<PromptItem>,
    #[serde(default = "default_true")]
    pub active_output_observed: bool,
    #[serde(default)]
    pub active_silent_since_ms: Option<i64>,
    pub draft_until_ms: i64,
    pub cancel_at_ms: Option<i64>,
    pub last_update_ms: i64,
    pub acknowledged_offset: u64,
    #[serde(default)]
    pub discard_until_newline: bool,
    pub next_message_seq: u64,
    pub open_messages: BTreeMap<String, u64>,
    /// Subagent sessions the harness reported, by session ID.
    #[serde(default)]
    pub(crate) subagents: BTreeMap<String, super::subagents::Tracked>,
    pub replay_role: Option<String>,
    pub probe_session_id: Option<String>,
    pub probe_started_ms: Option<i64>,
    pub login_pane_id: Option<String>,
    /// A sign-in terminal whose command exited 0. It stays open until the
    /// restarted agent opens a session, and is kept for review if it does not.
    #[serde(default)]
    pub unconfirmed_login_pane_id: Option<String>,
    /// The sign-in terminal replaces the account of a working session, so a
    /// failed or cancelled attempt leaves the session usable.
    #[serde(default)]
    pub(crate) relogin: bool,
    pub has_finished_turn: bool,
    pub permission_rules: BTreeMap<String, bool>,
    pub terminals: BTreeMap<String, super::terminals::Terminal>,
    #[serde(default)]
    pub(crate) harness_switch: Option<HarnessSwitchCheckpoint>,
    /// Identifies the engine's own continuation prompt without persisting its contents.
    #[serde(default)]
    pub(crate) internal_continuation: Option<InternalContinuation>,
    #[serde(skip)]
    pub(crate) replay_user: Option<ReplayedUserMessage>,
    /// Replay is inside the continuation turn, which the handover marker represents.
    #[serde(skip)]
    pub(crate) replay_continuation_turn: bool,
    #[serde(default)]
    pub(crate) retired_processes: Vec<RetiredAgentProcess>,
    #[serde(skip)]
    pub(crate) _clone_probe: super::idle::CloneProbe,
}

impl State {
    pub fn new(launch: HarnessLaunch, binary_digest: String, mode: Option<String>) -> Result<Self> {
        Ok(Self {
            binding: Binding::generate()?,
            hook_binding: Binding::generate()?,
            mcp_role: default_mcp_role(),
            mcp_enabled: true,
            launch,
            binary_digest,
            fingerprint: String::new(),
            client: Client::default(),
            initialized: None,
            phase: Phase::Initializing,
            auth_required: false,
            agent_sign_in: None,
            session_id: None,
            options: Options::default(),
            commands: Vec::new(),
            requested_options: mode
                .iter()
                .map(|mode| {
                    (
                        super::configuration::MODE_KEY.to_owned(),
                        ConfigValue::V0(mode.clone()),
                    )
                })
                .collect(),
            mode,
            operation: None,
            operation_user: None,
            operation_timed_out: false,
            initial_option: None,
            initial_config_failed: false,
            queue: VecDeque::new(),
            queue_revision: 0,
            transcribed_prompts: BTreeSet::new(),
            queue_paused: false,
            active: None,
            active_output_observed: true,
            active_silent_since_ms: None,
            draft_until_ms: 0,
            cancel_at_ms: None,
            last_update_ms: 0,
            acknowledged_offset: 0,
            discard_until_newline: false,
            next_message_seq: 0,
            open_messages: BTreeMap::new(),
            subagents: BTreeMap::new(),
            replay_role: None,
            probe_session_id: None,
            probe_started_ms: None,
            login_pane_id: None,
            unconfirmed_login_pane_id: None,
            relogin: false,
            has_finished_turn: false,
            permission_rules: BTreeMap::new(),
            terminals: BTreeMap::new(),
            harness_switch: None,
            internal_continuation: None,
            replay_user: None,
            replay_continuation_turn: false,
            retired_processes: Vec::new(),
            _clone_probe: Default::default(),
        })
    }

    pub(crate) fn ensure_binding(&mut self) -> Result<bool> {
        let mut changed = false;
        if self.binding.0.is_empty() {
            self.binding = Binding::generate()?;
            changed = true;
        }
        if self.hook_binding.0.is_empty() {
            self.hook_binding = Binding::generate()?;
            changed = true;
        }
        Ok(changed)
    }

    pub(crate) fn rotate_hook_binding(&mut self) -> Result<()> {
        self.hook_binding = Binding::generate()?;
        Ok(())
    }

    pub(crate) fn disable_mcp(&mut self) {
        self.mcp_enabled = false;
    }

    pub(crate) fn restart_for_task(
        &self,
        launch: HarnessLaunch,
        binary_digest: String,
        preset: &TaskExecutor,
    ) -> Result<Self> {
        let mode = preset.permission_mode.clone().or_else(|| self.mode.clone());
        let mut state = Self::new(launch, binary_digest, mode)?;
        state.binding = self.binding.clone();
        state.mcp_role = self.mcp_role.clone();
        state.mcp_enabled = self.mcp_enabled;
        state.next_message_seq = self.next_message_seq;
        for (name, value) in [("model", &preset.model), ("effort", &preset.effort)] {
            if let Some(value) = value {
                state
                    .requested_options
                    .insert(name.into(), ConfigValue::V0(value.clone()));
            }
        }
        Ok(state)
    }

    pub fn public(&self, now_ms: i64) -> AgentSession {
        let account = self.account_view();
        AgentSession {
            role: if self.mcp_role == Role::Chief {
                AgentPaneRole::Chief
            } else {
                AgentPaneRole::Worker
            },
            harness: self.launch.id.clone(),
            version: self
                .initialized
                .as_ref()
                .map_or_else(|| self.launch.version.clone(), |i| i.version.clone()),
            fingerprint: self.fingerprint.clone(),
            session_id: self.session_id.clone(),
            options: self.options.values.clone(),
            auth_methods: if self.auth_required || self.phase == Phase::NeedsLogin {
                self.initialized
                    .as_ref()
                    .map(|value| {
                        value
                            .auth_methods
                            .iter()
                            .map(|method| method.public.clone())
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            },
            operation: self.operation.clone(),
            queue: self
                .queue
                .iter()
                .map(|p| QueuedPromptSummary {
                    prompt_id: p.prompt_id.clone(),
                    origin: p.origin.clone(),
                    status: p.status.clone(),
                    queued_at: p.queued_at.clone(),
                    generation: p.generation,
                })
                .collect(),
            draft: self.draft_until_ms > now_ms,
            active_prompt_id: self.active.as_ref().map(|p| p.prompt_id.clone()),
            silent_since: self
                .active
                .as_ref()
                .and(self.active_silent_since_ms)
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            queue_paused: Some(self.queue_paused),
            harness_switch: self
                .harness_switch
                .as_ref()
                .map(|checkpoint| AgentHarnessSwitch {
                    from_harness: checkpoint.from_harness.clone(),
                    to_harness: checkpoint.to_harness.clone(),
                }),
            login: account.login,
            reauth_methods: account.reauth_methods,
            can_sign_out: account.can_sign_out,
        }
    }

    fn changed_queue(&mut self) -> Result<()> {
        self.queue_revision = self
            .queue_revision
            .checked_add(1)
            .ok_or_else(|| errors::internal("Queue revision exhausted"))?;
        Ok(())
    }

    pub fn enqueue(
        &mut self,
        text: String,
        origin: PromptOrigin,
        at: String,
    ) -> Result<PromptItem> {
        self.enqueue_with_attachments(text, origin, at, None)
    }

    pub fn enqueue_with_attachments(
        &mut self,
        text: String,
        origin: PromptOrigin,
        at: String,
        attachments: Option<Vec<PromptAttachment>>,
    ) -> Result<PromptItem> {
        self.enqueue_identified_with_attachments(
            Uuid::new_v4().to_string(),
            text,
            origin,
            at,
            attachments,
        )
    }

    pub(crate) fn enqueue_identified(
        &mut self,
        prompt_id: String,
        text: String,
        origin: PromptOrigin,
        at: String,
    ) -> Result<PromptItem> {
        self.enqueue_identified_with_attachments(prompt_id, text, origin, at, None)
    }

    fn enqueue_identified_with_attachments(
        &mut self,
        prompt_id: String,
        text: String,
        origin: PromptOrigin,
        at: String,
        attachments: Option<Vec<PromptAttachment>>,
    ) -> Result<PromptItem> {
        if text.is_empty() || text.len() > limits::PROMPT_BYTES {
            return Err(errors::invalid(
                "Prompt must contain 1 to 65536 UTF-8 bytes",
            ));
        }
        if self.queue.len() >= limits::PROMPT_QUEUE_COUNT {
            return Err(errors::pane_busy("Prompt queue is full"));
        }
        self.changed_queue()?;
        let prompt = PromptItem {
            prompt_id,
            text,
            origin,
            status: PromptStatus::Queued,
            queued_at: at,
            generation: None,
            attachments,
            agent_message: None,
        };
        self.queue.push_back(prompt.clone());
        self.prioritize_queue();
        Ok(prompt)
    }

    pub(crate) fn enqueue_feedback(
        &mut self,
        prompt_id: String,
        text: String,
        at: String,
    ) -> Result<PromptItem> {
        self.enqueue_identified(prompt_id, text, PromptOrigin::Automatic, at)
    }

    /// Takes over the queue of the state this one replaces, with the record of
    /// which of its prompts the transcript already holds.
    pub(crate) fn adopt_queue(&mut self, from: &State) {
        self.queue = from.queue.clone();
        self.queue_revision = from.queue_revision;
        self.transcribed_prompts = from.transcribed_prompts.clone();
    }

    pub(crate) fn restore_queue(&mut self, queue: Vec<PromptItem>) {
        self.queue = queue.into();
        self.prioritize_queue();
    }

    fn prioritize_queue(&mut self) {
        self.queue.make_contiguous().sort_by_key(prompt_priority);
    }

    pub fn remove(&mut self, id: &str) -> Result<bool> {
        let Some(index) = self.queue.iter().position(|p| p.prompt_id == id) else {
            return Ok(false);
        };
        self.changed_queue()?;
        self.queue.remove(index);
        self.transcribed_prompts.remove(id);
        Ok(true)
    }

    /// The driver also checks open requests before taking a prompt. Human
    /// messages take priority; draft presence holds only automatic work.
    pub fn take_next(
        &mut self,
        generation: u32,
        now_ms: i64,
        has_open_requests: bool,
    ) -> Result<Option<PromptItem>> {
        if self.phase != Phase::Ready
            || self.queue_paused
            || !self.requested_options.is_empty()
            || self.active.is_some()
            || self.operation.is_some()
            || has_open_requests
            || self.cancel_at_ms.is_some()
        {
            return Ok(None);
        }
        let human = self
            .queue
            .iter()
            .position(|p| p.origin == PromptOrigin::Human);
        let index = human
            .or_else(|| (self.draft_until_ms <= now_ms && !self.queue.is_empty()).then_some(0));
        let Some(index) = index else { return Ok(None) };
        self.changed_queue()?;
        let mut prompt = self
            .queue
            .remove(index)
            .ok_or_else(|| errors::internal("Selected prompt disappeared from the queue"))?;
        prompt.generation = Some(generation);
        prompt.status = PromptStatus::Active;
        self.active = Some(prompt.clone());
        Ok(Some(prompt))
    }

    pub fn queue_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
        now_ms: i64,
    ) -> Result<PaneQueueListResult> {
        if limit == 0 || limit > limits::PROMPT_QUEUE_COUNT {
            return Err(errors::invalid("Invalid queue page limit"));
        }
        let start = if let Some(cursor) = cursor {
            let (revision, index) = cursor
                .split_once('.')
                .ok_or_else(|| errors::invalid("Invalid queue cursor"))?;
            let revision = revision
                .parse::<u64>()
                .map_err(|_| errors::invalid("Invalid queue revision"))?;
            if revision != self.queue_revision {
                return Err(errors::resync_required(
                    "Queue changed; request the first page",
                ));
            }
            index
                .parse::<usize>()
                .map_err(|_| errors::invalid("Invalid queue position"))?
        } else {
            0
        };
        if start > self.queue.len() {
            return Err(errors::invalid("Queue cursor is out of range"));
        }
        let mut items = Vec::new();
        let mut bytes = 0;
        for item in self.queue.iter().skip(start).take(limit) {
            let size = errors::json_bytes(item)?.len() + 1;
            if bytes + size > limits::TRANSCRIPT_PAGE_BYTES - 256 {
                break;
            }
            bytes += size;
            items.push(item.clone());
        }
        let next = start + items.len();
        Ok(PaneQueueListResult {
            items,
            draft: self.draft_until_ms > now_ms,
            next_cursor: (next < self.queue.len())
                .then(|| format!("{}.{}", self.queue_revision, next)),
            paused: Some(self.queue_paused),
        })
    }

    /// Whether a native request or update comes from this pane's session or
    /// from a subagent session it reported.
    pub(crate) fn owns_session(&self, session: Option<&str>) -> bool {
        session.is_some_and(|session| {
            self.session_id.as_deref() == Some(session) || self.subagents.contains_key(session)
        })
    }

    pub fn message_sequence(&mut self) -> Result<u64> {
        let next = self.next_message_seq;
        self.next_message_seq = next
            .checked_add(1)
            .ok_or_else(|| errors::internal("Transcript sequence exhausted"))?;
        Ok(next)
    }
}

/// Options a pane can ask its session for. Checkpoints written before native
/// permission modes also carry a `permission` request no session can satisfy;
/// keeping it would hold the prompt queue forever.
const REQUESTABLE_OPTIONS: [&str; 3] = ["model", "effort", super::configuration::MODE_KEY];

fn requestable_options<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, ConfigValue>, D::Error> {
    let mut options = BTreeMap::<String, ConfigValue>::deserialize(deserializer)?;
    options.retain(|key, _| REQUESTABLE_OPTIONS.contains(&key.as_str()));
    Ok(options)
}

fn default_mcp_role() -> Role {
    Role::Agent
}

fn prompt_priority(item: &PromptItem) -> u8 {
    if item.origin == PromptOrigin::Human {
        0
    } else if item.prompt_id.starts_with("task-feedback:") {
        1
    } else {
        2
    }
}

#[cfg(test)]
pub(crate) mod tests;

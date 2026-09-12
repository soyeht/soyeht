use crate::errors::{self, Result};
use serde::{Deserialize, Serialize};
use soyeht_acp::{Client, config::Options, session::Initialization};
use soyeht_protocol::{
    AgentSession, ConfigOperation, ConfigValue, HarnessLaunch, PaneQueueListResult, PermissionMode,
    PromptItem, PromptOrigin, PromptStatus, QueuedPromptSummary, Role, limits,
};
use std::collections::{BTreeMap, VecDeque};
use uuid::Uuid;

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

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Binding(String);

impl std::fmt::Debug for Binding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Binding([redacted])")
    }
}

impl Binding {
    fn generate() -> Result<Self> {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub(crate) binding: Binding,
    #[serde(default = "default_mcp_role")]
    pub(crate) mcp_role: Role,
    #[serde(default = "mcp_enabled")]
    pub(crate) mcp_enabled: bool,
    pub launch: HarnessLaunch,
    pub binary_digest: String,
    pub fingerprint: String,
    pub client: Client,
    pub initialized: Option<Initialization>,
    pub phase: Phase,
    pub session_id: Option<String>,
    pub options: Options,
    pub permission: PermissionMode,
    pub previous_permission: Option<PermissionMode>,
    pub previous_effective_mode: Option<String>,
    pub pending_policy: Option<super::configuration::PolicyChange>,
    pub yolo: bool,
    pub effective_mode: Option<String>,
    pub operation: Option<ConfigOperation>,
    #[serde(default)]
    pub operation_timed_out: bool,
    pub operation_user: Option<super::pending::UserOperation>,
    pub requested_options: BTreeMap<String, ConfigValue>,
    pub initial_option: Option<String>,
    pub initial_config_failed: bool,
    #[serde(skip)]
    pub queue: VecDeque<PromptItem>,
    pub queue_revision: u64,
    #[serde(default)]
    pub queue_paused: bool,
    pub active: Option<PromptItem>,
    pub draft_until_ms: i64,
    pub cancel_at_ms: Option<i64>,
    pub last_update_ms: i64,
    pub acknowledged_offset: u64,
    #[serde(default)]
    pub discard_until_newline: bool,
    pub next_message_seq: u64,
    pub open_messages: BTreeMap<String, u64>,
    pub replay_role: Option<String>,
    pub probe_session_id: Option<String>,
    pub probe_started_ms: Option<i64>,
    pub login_pane_id: Option<String>,
    pub has_finished_turn: bool,
    pub permission_rules: BTreeMap<String, bool>,
    pub terminals: BTreeMap<String, super::terminals::Terminal>,
}

impl State {
    pub fn new(
        launch: HarnessLaunch,
        binary_digest: String,
        permission: PermissionMode,
    ) -> Result<Self> {
        let policy = match &permission {
            PermissionMode::Ask => "ask",
            PermissionMode::AcceptEdits => "accept_edits",
            PermissionMode::AllowAll => "allow_all",
        }
        .to_owned();
        Ok(Self {
            binding: Binding::generate()?,
            mcp_role: default_mcp_role(),
            mcp_enabled: true,
            launch,
            binary_digest,
            fingerprint: String::new(),
            client: Client::default(),
            initialized: None,
            phase: Phase::Initializing,
            session_id: None,
            options: Options::default(),
            permission,
            previous_permission: None,
            previous_effective_mode: None,
            pending_policy: None,
            yolo: false,
            effective_mode: None,
            operation: None,
            operation_user: None,
            operation_timed_out: false,
            requested_options: BTreeMap::from([("permission".into(), ConfigValue::V0(policy))]),
            initial_option: None,
            initial_config_failed: false,
            queue: VecDeque::new(),
            queue_revision: 0,
            queue_paused: false,
            active: None,
            draft_until_ms: 0,
            cancel_at_ms: None,
            last_update_ms: 0,
            acknowledged_offset: 0,
            discard_until_newline: false,
            next_message_seq: 0,
            open_messages: BTreeMap::new(),
            replay_role: None,
            probe_session_id: None,
            probe_started_ms: None,
            login_pane_id: None,
            has_finished_turn: false,
            permission_rules: BTreeMap::new(),
            terminals: BTreeMap::new(),
        })
    }

    pub(crate) fn ensure_binding(&mut self) -> Result<bool> {
        if self.binding.0.is_empty() {
            self.binding = Binding::generate()?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn disable_mcp(&mut self) {
        self.mcp_enabled = false;
    }

    pub fn public(&self, now_ms: i64) -> AgentSession {
        AgentSession {
            harness: self.launch.id.clone(),
            version: self
                .initialized
                .as_ref()
                .map_or_else(|| self.launch.version.clone(), |i| i.version.clone()),
            fingerprint: self.fingerprint.clone(),
            session_id: self.session_id.clone(),
            options: self.options.values.clone(),
            auth_methods: self
                .initialized
                .as_ref()
                .map(|value| {
                    value
                        .auth_methods
                        .iter()
                        .map(|method| method.public.clone())
                        .collect()
                })
                .unwrap_or_default(),
            permission: self.permission.clone(),
            yolo: self.yolo,
            previous_permission: self.previous_permission.clone(),
            effective_mode: self.effective_mode.clone(),
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
            queue_paused: Some(self.queue_paused),
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
            prompt_id: Uuid::new_v4().to_string(),
            text,
            origin,
            status: PromptStatus::Queued,
            queued_at: at,
            generation: None,
        };
        self.queue.push_back(prompt.clone());
        Ok(prompt)
    }

    pub fn remove(&mut self, id: &str) -> Result<bool> {
        let Some(index) = self.queue.iter().position(|p| p.prompt_id == id) else {
            return Ok(false);
        };
        self.changed_queue()?;
        self.queue.remove(index);
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

    pub fn message_sequence(&mut self) -> Result<u64> {
        let next = self.next_message_seq;
        self.next_message_seq = next
            .checked_add(1)
            .ok_or_else(|| errors::internal("Transcript sequence exhausted"))?;
        Ok(next)
    }
}

fn mcp_enabled() -> bool {
    true
}

fn default_mcp_role() -> Role {
    Role::Agent
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::ErrorKind;
    fn state() -> State {
        State::new(
            HarnessLaunch {
                id: "test".into(),
                name: "Test".into(),
                command: "/test".into(),
                args: vec![],
                version: "1".into(),
            },
            "digest".into(),
            PermissionMode::Ask,
        )
        .unwrap()
    }

    #[test]
    fn full_queue_fits_the_snapshot_and_pages_preserve_every_prompt_byte() {
        let mut state = state();
        for _ in 0..64 {
            state
                .enqueue(
                    "\0".repeat(limits::PROMPT_BYTES),
                    PromptOrigin::Human,
                    "now".into(),
                )
                .unwrap();
        }
        assert!(
            state
                .enqueue("overflow".into(), PromptOrigin::Human, "now".into())
                .is_err()
        );
        assert!(serde_json::to_vec(&state.public(0)).unwrap().len() < 65536);
        let mut cursor = None;
        let mut count = 0;
        loop {
            let page = state.queue_page(cursor.as_deref(), 64, 0).unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() <= limits::TRANSCRIPT_PAGE_BYTES);
            for item in page.items {
                assert_eq!(item.text, "\0".repeat(limits::PROMPT_BYTES));
                count += 1;
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(count, 64);
        let cursor = state.queue_page(None, 1, 0).unwrap().next_cursor.unwrap();
        let first = state.queue[0].prompt_id.clone();
        state.remove(&first).unwrap();
        assert_eq!(
            errors::kind(&state.queue_page(Some(&cursor), 1, 0).unwrap_err()),
            ErrorKind::ResyncRequired
        );
        assert!(
            state
                .enqueue("🌍".repeat(16385), PromptOrigin::Human, "now".into())
                .is_err()
        );
    }

    #[test]
    fn public_state_keeps_authentication_choices_across_snapshot_reconnection() {
        let mut state = state();
        state.phase = Phase::NeedsLogin;
        state.initialized = Some(
            Initialization::parse(&serde_json::json!({
                "protocolVersion": 1,
                "authMethods": [{"id": "login", "name": "Sign in", "type": "terminal"}]
            }))
            .unwrap(),
        );
        let public = state.public(0);
        assert_eq!(public.auth_methods.len(), 1);
        assert_eq!(public.auth_methods[0].id, "login");
        assert!(public.auth_methods[0].terminal);
    }

    #[test]
    fn checkpoints_from_before_reviewer_isolation_keep_the_task_server_enabled() {
        let serialized = serde_json::to_value(state()).unwrap();
        let mut object = serialized.as_object().unwrap().clone();
        object.remove("mcp_enabled");

        let restored: State = serde_json::from_value(serde_json::Value::Object(object)).unwrap();

        assert!(restored.mcp_enabled);
    }

    #[test]
    fn restored_queue_obeys_draft_priority_open_requests_and_serial_changes() {
        let mut state = state();
        state.phase = Phase::Ready;
        state.draft_until_ms = 1000;
        let automatic = state
            .enqueue("automatic".into(), PromptOrigin::Automatic, "now".into())
            .unwrap();
        assert!(state.take_next(1, 1, false).unwrap().is_none());
        let human = state
            .enqueue("human".into(), PromptOrigin::Human, "now".into())
            .unwrap();
        assert!(state.take_next(1, 1, false).unwrap().is_none());
        state.requested_options.clear(); // Native initial configuration completed.
        assert!(state.take_next(1, 1, true).unwrap().is_none());
        assert_eq!(
            state.take_next(1, 1, false).unwrap().unwrap().prompt_id,
            human.prompt_id
        );
        let mut restored: State =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        restored.queue = state.queue.clone(); // Hydrated separately from ordered queue rows.
        assert!(restored.take_next(1, 2000, false).unwrap().is_none());
        restored.active = None;
        assert_eq!(
            restored
                .take_next(2, 2000, false)
                .unwrap()
                .unwrap()
                .prompt_id,
            automatic.prompt_id
        );
        assert_eq!(restored.active.unwrap().generation, Some(2));
    }
}

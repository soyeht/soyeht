//! IO boundaries used by engine rules.
//!
//! The composition root supplies the durable store, process supervisor and
//! wall clock. Rule helpers depend on these traits so their decisions can be
//! exercised without opening a database, socket or child process.

use crate::agents::state::State;
use serde::de::DeserializeOwned;
use serde_json::Value;
use soyeht_protocol::{
    PaneTranscriptResult, ProbeCapability, PromptItem, TranscriptMessage, Workspace,
};
use soyeht_ptyd::{
    client::Client as SupervisorClient,
    wire::{Request, Response, Spawn},
};
use soyeht_store::web::{WebPermissionRequest, WebSessionGrant};
use soyeht_store::{
    agents::{AgentModelRefusal, HarnessReadiness, NativeRequest, Outgoing, PromptAttachment},
    database::{Change, Mutation, Result as StoreResult, Store},
    domain::{ClientFocus, DeviceIdentity, IdempotencyReceipt, InputIntent, PaneRecord},
};
use std::io;

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;

    fn now_unix(&self) -> i64 {
        self.now_ms().div_euclid(1_000)
    }

    fn timestamp(&self) -> String {
        chrono::DateTime::from_timestamp_millis(self.now_ms())
            .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
            .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
    }
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}

pub trait SupervisorPort: Send {
    fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response>;
    fn supervisor_id(&self) -> &str;
    fn supervisor_generation(&self) -> u32;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortScanObservation {
    pub listeners: Vec<u8>,
    pub directories: Vec<u8>,
}

pub trait PortScanPort: Send {
    fn scan(&mut self) -> io::Result<PortScanObservation>;
}

impl SupervisorPort for SupervisorClient {
    fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response> {
        SupervisorClient::request(self, request)
    }

    fn supervisor_id(&self) -> &str {
        &self.supervisor_id
    }

    fn supervisor_generation(&self) -> u32 {
        self.supervisor_generation
    }
}

pub fn call_supervisor(
    supervisor: &mut (impl SupervisorPort + ?Sized),
    request: Request,
) -> crate::errors::Result<Response> {
    match supervisor.request(request) {
        Ok(Response::Error { kind, message }) => Err(crate::errors::Error::Supervisor(
            soyeht_ptyd::Error::Remote { kind, message },
        )),
        Ok(response) => Ok(response),
        Err(error) => Err(crate::errors::Error::Supervisor(error)),
    }
}

/// The store surface read by engine rules.
///
/// Atomic change functions still receive the store's opaque [`Change`]. That
/// capability exposes named domain operations, never a driver connection.
pub trait StorePort {
    fn store_id(&self) -> &str;

    fn mutate<F>(
        &mut self,
        scope: &str,
        key: &str,
        fingerprint: &str,
        apply: F,
    ) -> StoreResult<Mutation>
    where
        F: FnOnce(&Change<'_>) -> StoreResult<(Value, Vec<(String, Value)>)>;

    fn mutate_as<F>(
        &mut self,
        scope: &str,
        key: &str,
        fingerprint: &str,
        actor: Option<&Value>,
        apply: F,
    ) -> StoreResult<Mutation>
    where
        F: FnOnce(&Change<'_>) -> StoreResult<(Value, Vec<(String, Value)>)>;

    fn workspace(&self, id: &str) -> StoreResult<Option<Workspace>>;
    fn panes(&self) -> StoreResult<Vec<PaneRecord>>;
    fn setting(&self, key: &str) -> StoreResult<Option<Value>>;
    fn input_intent(&self, scope: &str, key: &str) -> StoreResult<Option<InputIntent>>;
    fn record_input_intent(
        &mut self,
        scope: &str,
        key: &str,
        intent: &InputIntent,
    ) -> StoreResult<()>;
    fn input_intent_operation_pending(&self, operation_key: &str) -> StoreResult<bool>;
    fn pane_launch(&self, pane_id: &str) -> StoreResult<Option<Spawn>>;
    fn has_pane_launch(&self, pane_id: &str) -> StoreResult<bool>;
    fn client_focus(&self, scope: &str) -> StoreResult<Option<ClientFocus>>;
    fn idempotency_receipt(
        &self,
        scope: &str,
        key: &str,
    ) -> StoreResult<Option<IdempotencyReceipt>>;
    fn device_identity(&self, token_hash: &[u8]) -> StoreResult<Option<DeviceIdentity>>;
    fn active_pair_requests<T: DeserializeOwned>(&self, now: i64) -> StoreResult<Vec<T>>;
    fn active_pair_request<T: DeserializeOwned>(
        &self,
        id: &str,
        now: i64,
    ) -> StoreResult<Option<T>>;
    fn active_pair_window_expiry(&self, code: &str, now: i64) -> StoreResult<Option<i64>>;
    fn pair_window_active(&self, code: &str, now: i64) -> StoreResult<bool>;
    fn device<T: DeserializeOwned>(&self, id: &str) -> StoreResult<Option<T>>;
    fn agent_runtime(&self, pane: &str) -> StoreResult<Option<State>>;
    fn agent_queue(&self, pane: &str) -> StoreResult<Vec<PromptItem>>;
    fn agent_prompt_attachments(&self, prompt: &str) -> StoreResult<Vec<PromptAttachment>>;
    fn agent_open_requests(&self, pane: &str) -> StoreResult<Vec<NativeRequest>>;
    fn agent_request(&self, request_id: &str) -> StoreResult<Option<NativeRequest>>;
    fn agent_request_at(
        &self,
        pane: &str,
        generation: u32,
        sequence: u64,
    ) -> StoreResult<Option<NativeRequest>>;
    fn agent_outbox(&self, pane: &str) -> StoreResult<Vec<Outgoing>>;
    fn agent_outgoing(&self, operation_key: &str) -> StoreResult<Option<Outgoing>>;
    fn agent_message(&self, pane: &str, sequence: u64) -> StoreResult<Option<TranscriptMessage>>;
    fn agent_transcript(
        &self,
        pane: &str,
        before: Option<u64>,
        limit: usize,
    ) -> StoreResult<PaneTranscriptResult>;
    fn agent_probe(&self, fingerprint: &str, method: &str) -> StoreResult<Option<ProbeCapability>>;
    fn harness_readiness(&self, harness: &str) -> StoreResult<Option<HarnessReadiness>>;
    fn agent_model_refusals(&self, harness: &str) -> StoreResult<Vec<AgentModelRefusal>>;
    fn web_session_grant(
        &self,
        workspace_id: &str,
        domain: &str,
        task_id: Option<&str>,
    ) -> StoreResult<Option<WebSessionGrant>>;
    fn open_web_permission(
        &self,
        pane_id: &str,
        kind: &soyeht_protocol::PermissionRequestKind,
        domain: &str,
        task_id: Option<&str>,
    ) -> StoreResult<Option<WebPermissionRequest>>;
    fn web_permission_request(&self, request_id: &str)
    -> StoreResult<Option<WebPermissionRequest>>;
    fn web_permission_for_operation(
        &self,
        pane_id: &str,
        kind: &soyeht_protocol::PermissionRequestKind,
        operation_key: &str,
    ) -> StoreResult<Option<WebPermissionRequest>>;
    fn expired_web_permissions(&self, now_ms: i64) -> StoreResult<Vec<WebPermissionRequest>>;
}

impl StorePort for Store {
    fn store_id(&self) -> &str {
        &self.db_id
    }

    fn mutate<F>(
        &mut self,
        scope: &str,
        key: &str,
        fingerprint: &str,
        apply: F,
    ) -> StoreResult<Mutation>
    where
        F: FnOnce(&Change<'_>) -> StoreResult<(Value, Vec<(String, Value)>)>,
    {
        Store::mutate(self, scope, key, fingerprint, apply)
    }

    fn mutate_as<F>(
        &mut self,
        scope: &str,
        key: &str,
        fingerprint: &str,
        actor: Option<&Value>,
        apply: F,
    ) -> StoreResult<Mutation>
    where
        F: FnOnce(&Change<'_>) -> StoreResult<(Value, Vec<(String, Value)>)>,
    {
        Store::mutate_as(self, scope, key, fingerprint, actor, apply)
    }

    fn workspace(&self, id: &str) -> StoreResult<Option<Workspace>> {
        Store::workspace(self, id)
    }

    fn panes(&self) -> StoreResult<Vec<PaneRecord>> {
        Store::panes(self)
    }

    fn setting(&self, key: &str) -> StoreResult<Option<Value>> {
        Store::setting(self, key)
    }

    fn input_intent(&self, scope: &str, key: &str) -> StoreResult<Option<InputIntent>> {
        Store::input_intent(self, scope, key)
    }

    fn record_input_intent(
        &mut self,
        scope: &str,
        key: &str,
        intent: &InputIntent,
    ) -> StoreResult<()> {
        Store::record_input_intent(self, scope, key, intent)
    }

    fn input_intent_operation_pending(&self, operation_key: &str) -> StoreResult<bool> {
        Store::input_intent_operation_pending(self, operation_key)
    }

    fn pane_launch(&self, pane_id: &str) -> StoreResult<Option<Spawn>> {
        Store::pane_launch(self, pane_id)
    }

    fn has_pane_launch(&self, pane_id: &str) -> StoreResult<bool> {
        Store::has_pane_launch(self, pane_id)
    }

    fn client_focus(&self, scope: &str) -> StoreResult<Option<ClientFocus>> {
        Store::client_focus(self, scope)
    }

    fn idempotency_receipt(
        &self,
        scope: &str,
        key: &str,
    ) -> StoreResult<Option<IdempotencyReceipt>> {
        Store::idempotency_receipt(self, scope, key)
    }

    fn device_identity(&self, token_hash: &[u8]) -> StoreResult<Option<DeviceIdentity>> {
        Store::device_identity(self, token_hash)
    }

    fn active_pair_requests<T: DeserializeOwned>(&self, now: i64) -> StoreResult<Vec<T>> {
        Store::active_pair_requests(self, now)
    }

    fn active_pair_request<T: DeserializeOwned>(
        &self,
        id: &str,
        now: i64,
    ) -> StoreResult<Option<T>> {
        Store::active_pair_request(self, id, now)
    }

    fn active_pair_window_expiry(&self, code: &str, now: i64) -> StoreResult<Option<i64>> {
        Store::active_pair_window_expiry(self, code, now)
    }

    fn pair_window_active(&self, code: &str, now: i64) -> StoreResult<bool> {
        Store::pair_window_active(self, code, now)
    }

    fn device<T: DeserializeOwned>(&self, id: &str) -> StoreResult<Option<T>> {
        Store::device(self, id)
    }

    fn agent_runtime(&self, pane: &str) -> StoreResult<Option<State>> {
        soyeht_store::agents::runtime(self, pane)
    }

    fn agent_queue(&self, pane: &str) -> StoreResult<Vec<PromptItem>> {
        soyeht_store::agents::queue(self, pane)
    }

    fn agent_prompt_attachments(&self, prompt: &str) -> StoreResult<Vec<PromptAttachment>> {
        soyeht_store::agents::prompt_attachments(self, prompt)
    }

    fn agent_open_requests(&self, pane: &str) -> StoreResult<Vec<NativeRequest>> {
        soyeht_store::agents::open_requests(self, pane)
    }

    fn agent_request(&self, request_id: &str) -> StoreResult<Option<NativeRequest>> {
        soyeht_store::agents::request(self, request_id)
    }

    fn agent_request_at(
        &self,
        pane: &str,
        generation: u32,
        sequence: u64,
    ) -> StoreResult<Option<NativeRequest>> {
        soyeht_store::agents::request_at(self, pane, generation, sequence)
    }

    fn agent_outbox(&self, pane: &str) -> StoreResult<Vec<Outgoing>> {
        soyeht_store::agents::outbox(self, pane)
    }

    fn agent_outgoing(&self, operation_key: &str) -> StoreResult<Option<Outgoing>> {
        soyeht_store::agents::outgoing(self, operation_key)
    }

    fn agent_message(&self, pane: &str, sequence: u64) -> StoreResult<Option<TranscriptMessage>> {
        soyeht_store::agents::message(self, pane, sequence)
    }

    fn agent_transcript(
        &self,
        pane: &str,
        before: Option<u64>,
        limit: usize,
    ) -> StoreResult<PaneTranscriptResult> {
        soyeht_store::agents::transcript(self, pane, before, limit)
    }

    fn agent_probe(&self, fingerprint: &str, method: &str) -> StoreResult<Option<ProbeCapability>> {
        soyeht_store::agents::probe(self, fingerprint, method)
    }

    fn harness_readiness(&self, harness: &str) -> StoreResult<Option<HarnessReadiness>> {
        soyeht_store::agents::harness_readiness(self, harness)
    }

    fn agent_model_refusals(&self, harness: &str) -> StoreResult<Vec<AgentModelRefusal>> {
        soyeht_store::agents::model_refusals(self, harness)
    }

    fn web_session_grant(
        &self,
        workspace_id: &str,
        domain: &str,
        task_id: Option<&str>,
    ) -> StoreResult<Option<WebSessionGrant>> {
        Store::web_session_grant(self, workspace_id, domain, task_id)
    }

    fn open_web_permission(
        &self,
        pane_id: &str,
        kind: &soyeht_protocol::PermissionRequestKind,
        domain: &str,
        task_id: Option<&str>,
    ) -> StoreResult<Option<WebPermissionRequest>> {
        Store::open_web_permission(self, pane_id, kind, domain, task_id)
    }

    fn web_permission_request(
        &self,
        request_id: &str,
    ) -> StoreResult<Option<WebPermissionRequest>> {
        Store::web_permission_request(self, request_id)
    }

    fn web_permission_for_operation(
        &self,
        pane_id: &str,
        kind: &soyeht_protocol::PermissionRequestKind,
        operation_key: &str,
    ) -> StoreResult<Option<WebPermissionRequest>> {
        Store::web_permission_for_operation(self, pane_id, kind, operation_key)
    }

    fn expired_web_permissions(&self, now_ms: i64) -> StoreResult<Vec<WebPermissionRequest>> {
        Store::expired_web_permissions(self, now_ms)
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use soyeht_store::database::Error as StoreError;
    use std::collections::{BTreeMap, VecDeque};

    #[derive(Debug)]
    pub struct FixedClock(pub i64);

    impl Clock for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    #[derive(Default)]
    pub struct FakeSupervisor {
        pub requests: Vec<Value>,
        pub responses: VecDeque<soyeht_ptyd::Result<Response>>,
        pub id: String,
        pub generation: u32,
    }

    impl SupervisorPort for FakeSupervisor {
        fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response> {
            self.requests.push(serde_json::to_value(request)?);
            self.responses
                .pop_front()
                .unwrap_or(Err(soyeht_ptyd::Error::InvalidResponse))
        }

        fn supervisor_id(&self) -> &str {
            &self.id
        }

        fn supervisor_generation(&self) -> u32 {
            self.generation
        }
    }

    #[derive(Default)]
    pub struct MemoryStore {
        pub workspaces: BTreeMap<String, Workspace>,
        pub settings: BTreeMap<String, Value>,
        pub intents: BTreeMap<(String, String), InputIntent>,
        pub pair_requests: BTreeMap<String, Value>,
        pub pair_windows: BTreeMap<String, i64>,
        pub open_requests: BTreeMap<String, Vec<NativeRequest>>,
        pub probes: BTreeMap<(String, String), ProbeCapability>,
        pub harness_readiness: BTreeMap<String, HarnessReadiness>,
        pub model_refusals: BTreeMap<(String, String), AgentModelRefusal>,
    }

    impl StorePort for MemoryStore {
        fn store_id(&self) -> &str {
            "memory-store"
        }

        fn mutate<F>(
            &mut self,
            _scope: &str,
            _key: &str,
            _fingerprint: &str,
            _apply: F,
        ) -> StoreResult<Mutation>
        where
            F: FnOnce(&Change<'_>) -> StoreResult<(Value, Vec<(String, Value)>)>,
        {
            Err(StoreError::TestFixture("fake transaction not configured"))
        }

        fn mutate_as<F>(
            &mut self,
            _scope: &str,
            _key: &str,
            _fingerprint: &str,
            _actor: Option<&Value>,
            _apply: F,
        ) -> StoreResult<Mutation>
        where
            F: FnOnce(&Change<'_>) -> StoreResult<(Value, Vec<(String, Value)>)>,
        {
            Err(StoreError::TestFixture("fake transaction not configured"))
        }

        fn workspace(&self, id: &str) -> StoreResult<Option<Workspace>> {
            Ok(self.workspaces.get(id).cloned())
        }

        fn panes(&self) -> StoreResult<Vec<PaneRecord>> {
            Ok(Vec::new())
        }

        fn setting(&self, key: &str) -> StoreResult<Option<Value>> {
            Ok(self.settings.get(key).cloned())
        }

        fn input_intent(&self, scope: &str, key: &str) -> StoreResult<Option<InputIntent>> {
            Ok(self.intents.get(&(scope.into(), key.into())).cloned())
        }

        fn record_input_intent(
            &mut self,
            scope: &str,
            key: &str,
            intent: &InputIntent,
        ) -> StoreResult<()> {
            self.intents
                .insert((scope.into(), key.into()), intent.clone());
            Ok(())
        }

        fn input_intent_operation_pending(&self, operation_key: &str) -> StoreResult<bool> {
            Ok(self
                .intents
                .values()
                .any(|intent| intent.operation_key == operation_key))
        }

        fn pane_launch(&self, _pane_id: &str) -> StoreResult<Option<Spawn>> {
            Ok(None)
        }

        fn has_pane_launch(&self, _pane_id: &str) -> StoreResult<bool> {
            Ok(false)
        }

        fn client_focus(&self, _scope: &str) -> StoreResult<Option<ClientFocus>> {
            Ok(None)
        }

        fn idempotency_receipt(
            &self,
            _scope: &str,
            _key: &str,
        ) -> StoreResult<Option<IdempotencyReceipt>> {
            Ok(None)
        }

        fn device_identity(&self, _token_hash: &[u8]) -> StoreResult<Option<DeviceIdentity>> {
            Ok(None)
        }

        fn active_pair_requests<T: DeserializeOwned>(&self, now: i64) -> StoreResult<Vec<T>> {
            self.pair_requests
                .values()
                .filter(|value| {
                    value["expires"]
                        .as_i64()
                        .is_some_and(|expires| expires > now)
                })
                .cloned()
                .map(|value| serde_json::from_value(value).map_err(Into::into))
                .collect()
        }

        fn active_pair_request<T: DeserializeOwned>(
            &self,
            id: &str,
            now: i64,
        ) -> StoreResult<Option<T>> {
            self.pair_requests
                .get(id)
                .filter(|value| {
                    value["expires"]
                        .as_i64()
                        .is_some_and(|expires| expires > now)
                })
                .cloned()
                .map(|value| serde_json::from_value(value).map_err(Into::into))
                .transpose()
        }

        fn active_pair_window_expiry(&self, code: &str, now: i64) -> StoreResult<Option<i64>> {
            Ok(self
                .pair_windows
                .get(code)
                .copied()
                .filter(|expires| *expires > now))
        }

        fn pair_window_active(&self, code: &str, now: i64) -> StoreResult<bool> {
            Ok(self
                .pair_windows
                .get(code)
                .is_some_and(|expires| *expires > now))
        }

        fn device<T: DeserializeOwned>(&self, _id: &str) -> StoreResult<Option<T>> {
            Ok(None)
        }

        fn agent_runtime(&self, _pane: &str) -> StoreResult<Option<State>> {
            Ok(None)
        }

        fn agent_queue(&self, _pane: &str) -> StoreResult<Vec<PromptItem>> {
            Ok(Vec::new())
        }

        fn agent_prompt_attachments(&self, _prompt: &str) -> StoreResult<Vec<PromptAttachment>> {
            Ok(Vec::new())
        }

        fn agent_open_requests(&self, pane: &str) -> StoreResult<Vec<NativeRequest>> {
            Ok(self.open_requests.get(pane).cloned().unwrap_or_default())
        }

        fn agent_request(&self, request_id: &str) -> StoreResult<Option<NativeRequest>> {
            Ok(self
                .open_requests
                .values()
                .flatten()
                .find(|request| request.request_id == request_id)
                .cloned())
        }

        fn agent_request_at(
            &self,
            pane: &str,
            generation: u32,
            sequence: u64,
        ) -> StoreResult<Option<NativeRequest>> {
            Ok(self.open_requests.get(pane).and_then(|requests| {
                requests
                    .iter()
                    .find(|request| {
                        request.generation == generation && request.source_seq == sequence
                    })
                    .cloned()
            }))
        }

        fn agent_outbox(&self, _pane: &str) -> StoreResult<Vec<Outgoing>> {
            Ok(Vec::new())
        }

        fn agent_outgoing(&self, _operation_key: &str) -> StoreResult<Option<Outgoing>> {
            Ok(None)
        }

        fn agent_message(
            &self,
            _pane: &str,
            _sequence: u64,
        ) -> StoreResult<Option<TranscriptMessage>> {
            Ok(None)
        }

        fn agent_transcript(
            &self,
            _pane: &str,
            _before: Option<u64>,
            _limit: usize,
        ) -> StoreResult<PaneTranscriptResult> {
            Ok(PaneTranscriptResult {
                items: Vec::new(),
                next_cursor: None,
            })
        }

        fn agent_probe(
            &self,
            fingerprint: &str,
            method: &str,
        ) -> StoreResult<Option<ProbeCapability>> {
            Ok(self
                .probes
                .get(&(fingerprint.into(), method.into()))
                .cloned())
        }

        fn harness_readiness(&self, harness: &str) -> StoreResult<Option<HarnessReadiness>> {
            Ok(self.harness_readiness.get(harness).cloned())
        }

        fn agent_model_refusals(&self, harness: &str) -> StoreResult<Vec<AgentModelRefusal>> {
            Ok(self
                .model_refusals
                .values()
                .filter(|refusal| refusal.harness_id == harness)
                .cloned()
                .collect())
        }

        fn web_session_grant(
            &self,
            _workspace_id: &str,
            _domain: &str,
            _task_id: Option<&str>,
        ) -> StoreResult<Option<WebSessionGrant>> {
            Ok(None)
        }

        fn open_web_permission(
            &self,
            _pane_id: &str,
            _kind: &soyeht_protocol::PermissionRequestKind,
            _domain: &str,
            _task_id: Option<&str>,
        ) -> StoreResult<Option<WebPermissionRequest>> {
            Ok(None)
        }

        fn web_permission_request(
            &self,
            _request_id: &str,
        ) -> StoreResult<Option<WebPermissionRequest>> {
            Ok(None)
        }

        fn web_permission_for_operation(
            &self,
            _pane_id: &str,
            _kind: &soyeht_protocol::PermissionRequestKind,
            _operation_key: &str,
        ) -> StoreResult<Option<WebPermissionRequest>> {
            Ok(None)
        }

        fn expired_web_permissions(&self, _now_ms: i64) -> StoreResult<Vec<WebPermissionRequest>> {
            Ok(Vec::new())
        }
    }
}

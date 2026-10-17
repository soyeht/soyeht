use super::host::WebWaiter;
use crate::{
    agents::questions::QUESTION_TIMEOUT_MS,
    core::{Core, write_pane},
    errors::{self, Result},
    model,
    ports::StorePort,
    tasks::ports::TaskStorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Actor, DecisionBy, PaneMode, PanePermissionAnswerParams, PanePermissionAnswerResult,
    PermissionOption, PermissionOptionKind, PermissionRequest, PermissionRequestKind,
    PermissionResult, Role, WebCommandParams, WebSessionState, event, method,
};
use soyeht_store::web::{WebPermissionRequest, WebSessionGrant, WebSessionGrantScope};
use soyeht_web::NormalizedUrl;
use uuid::Uuid;

const INTERNAL_ACCESS_KEY: &str = "_soyehtAccess";

#[derive(Clone)]
enum SessionAccess {
    Human,
    Incognito,
    Probe,
    Granted,
}

#[derive(Clone)]
pub(super) struct WebPermission {
    agent: bool,
    domain: String,
    task_id: Option<String>,
    session: SessionAccess,
    evaluation: bool,
    pub(super) request_id: Option<String>,
    pub(super) expires_ms: Option<i64>,
}

enum PermissionDecision {
    Allowed,
    Denied,
    Pending(Box<WebPermissionRequest>),
}

impl WebPermission {
    pub(super) fn driving_domain(&self) -> Option<&str> {
        self.agent.then_some(self.domain.as_str())
    }

    pub(super) fn apply_to(&self, params: &mut WebCommandParams) -> Result<()> {
        let args = params
            .args
            .as_object_mut()
            .ok_or_else(|| errors::invalid("Web command arguments must be an object"))?;
        let session = match self.session {
            SessionAccess::Human => "human",
            SessionAccess::Incognito => "incognito",
            SessionAccess::Probe => "probe",
            SessionAccess::Granted => "granted",
        };
        args.insert(
            INTERNAL_ACCESS_KEY.into(),
            json!({"session":session,"evaluation":self.evaluation}),
        );
        Ok(())
    }
}

impl Core {
    pub(super) fn bound_web_workspace(&self, actor: &Actor) -> Result<Option<String>> {
        if !matches!(actor.role, Role::Agent | Role::Chief) {
            return Ok(None);
        }
        let pane_id = actor
            .pane_id
            .as_deref()
            .ok_or_else(|| errors::unauthorized("Agent binding has no pane identity"))?;
        let pane = self
            .panes
            .get(pane_id)
            .filter(|pane| !pane.stored.closed && pane.stored.pane.mode == PaneMode::Agent)
            .ok_or_else(|| errors::unauthorized("Bound agent pane is unavailable"))?;
        Ok(Some(pane.stored.pane.workspace_id.clone()))
    }

    pub(super) fn authorize_web_workspace(&self, actor: &Actor, workspace_id: &str) -> Result<()> {
        if self
            .bound_web_workspace(actor)?
            .is_some_and(|bound| bound != workspace_id)
        {
            return Err(errors::forbidden_role(
                "Bound agents may use web panes only in their workspace",
            ));
        }
        Ok(())
    }

    pub(super) fn web_permission_plan(
        &mut self,
        caller: &str,
        method: &str,
        params: &WebCommandParams,
    ) -> Result<WebPermission> {
        let actor = self
            .clients
            .get(caller)
            .and_then(|client| client.actor.clone())
            .ok_or_else(|| errors::unauthorized("Web command caller is disconnected"))?;
        let pane = self
            .panes
            .get(&params.pane_id)
            .filter(|running| !running.stored.closed)
            .ok_or_else(|| errors::not_found("Web pane not found"))?;
        self.authorize_web_workspace(&actor, &pane.stored.pane.workspace_id)?;
        let agent = matches!(actor.role, Role::Agent | Role::Chief);
        let domain = if agent {
            let url = pane
                .stored
                .pane
                .url
                .as_deref()
                .ok_or_else(|| errors::invalid("Web pane has no current URL"))?;
            NormalizedUrl::parse(url)?.host().to_owned()
        } else {
            String::new()
        };
        let task_id = self.web_task_id(actor.pane_id.as_deref())?;
        let incognito = pane.stored.pane.incognito == Some(true);
        let granted = agent
            && !incognito
            && StorePort::web_session_grant(
                &self.store,
                &pane.stored.pane.workspace_id,
                &domain,
                task_id.as_deref(),
            )
            .map_err(errors::storage)?
            .is_some();
        let mut permission = WebPermission {
            agent,
            domain,
            task_id,
            session: session_access(agent, incognito, granted),
            evaluation: !agent && method == method::WEB_EVALUATE,
            request_id: None,
            expires_ms: None,
        };
        if agent && method == method::WEB_EVALUATE {
            self.apply_permission_decision(
                &mut permission,
                params,
                PermissionRequestKind::WebEvaluate,
            )?;
        }
        Ok(permission)
    }

    fn web_task_id(&self, agent_pane: Option<&str>) -> Result<Option<String>> {
        agent_pane
            .map(|pane| TaskStorePort::active_task_for_pane(&self.store, pane))
            .transpose()
            .map_err(errors::storage)
            .map(|task| task.flatten().map(|task| task.task_id))
    }

    fn apply_permission_decision(
        &mut self,
        permission: &mut WebPermission,
        params: &WebCommandParams,
        kind: PermissionRequestKind,
    ) -> Result<()> {
        match self.ensure_web_permission(permission, params, kind)? {
            PermissionDecision::Allowed => permission.evaluation = true,
            PermissionDecision::Denied => {
                return Err(errors::unauthorized("Web permission was denied"));
            }
            PermissionDecision::Pending(request) => {
                permission.request_id = Some(request.request_id);
                permission.expires_ms = Some(request.expires_ms);
            }
        }
        Ok(())
    }

    fn ensure_web_permission(
        &mut self,
        permission: &WebPermission,
        params: &WebCommandParams,
        kind: PermissionRequestKind,
    ) -> Result<PermissionDecision> {
        let existing = if kind == PermissionRequestKind::WebEvaluate {
            StorePort::web_permission_for_operation(
                &self.store,
                &params.pane_id,
                &kind,
                &params.operation_key,
            )
        } else {
            StorePort::open_web_permission(
                &self.store,
                &params.pane_id,
                &kind,
                &permission.domain,
                permission.task_id.as_deref(),
            )
        }
        .map_err(errors::storage)?;
        if let Some(request) = existing {
            return Ok(permission_decision(request));
        }
        let request = self.new_web_permission(permission, params, kind)?;
        self.persist_web_permission(&request)?;
        Ok(PermissionDecision::Pending(Box::new(request)))
    }

    fn new_web_permission(
        &self,
        permission: &WebPermission,
        params: &WebCommandParams,
        kind: PermissionRequestKind,
    ) -> Result<WebPermissionRequest> {
        let pane = &self.panes[&params.pane_id].stored.pane;
        let expires_ms = self.now_ms().saturating_add(QUESTION_TIMEOUT_MS);
        let expires_at = chrono::DateTime::from_timestamp_millis(expires_ms)
            .ok_or_else(|| errors::internal("Permission deadline is outside timestamp range"))?
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let request_id = Uuid::new_v4().to_string();
        let public = PermissionRequest {
            request_id: request_id.clone(),
            pane_id: params.pane_id.clone(),
            generation: pane.generation,
            prompt_id: params.operation_key.clone(),
            title: permission_title(&kind).into(),
            detail: permission.domain.clone(),
            presentation: None,
            options: permission_options(&kind, permission.task_id.is_some()),
            expires_at,
            kind: Some(kind.clone()),
            domain: Some(permission.domain.clone()),
        };
        Ok(WebPermissionRequest {
            request_id,
            pane_id: params.pane_id.clone(),
            workspace_id: pane.workspace_id.clone(),
            generation: pane.generation,
            task_id: permission.task_id.clone(),
            kind,
            domain: permission.domain.clone(),
            public,
            expires_ms,
            result: None,
        })
    }

    fn persist_web_permission(&mut self, request: &WebPermissionRequest) -> Result<()> {
        let mut saved = self.panes[&request.pane_id].stored.clone();
        if request.kind == PermissionRequestKind::WebSession {
            saved.pane.web_session = Some(WebSessionState::PermissionRequired);
        }
        let public = serde_json::to_value(&request.public)
            .map_err(|_| errors::internal("Permission serialization failed"))?;
        let mut events = vec![(event::PANE_PERMISSION_REQUESTED.into(), public)];
        if request.kind == PermissionRequestKind::WebSession {
            events.push(model::pane_updated(&saved.pane)?);
        }
        let request_for_write = request.clone();
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine",
            &format!("web-permission-open:{}", request.request_id),
            "web.permission.open",
            |change| {
                change.save_web_permission_request(&request_for_write)?;
                write_pane(change, &saved)?;
                Ok((json!({}), events))
            },
        )
        .map_err(errors::storage)?;
        if request.kind == PermissionRequestKind::WebSession {
            self.panes
                .get_mut(&request.pane_id)
                .ok_or_else(|| errors::internal("Web pane disappeared"))?
                .stored = saved;
        }
        self.publish(&mutation.events)
    }

    pub(super) fn web_session_required(&mut self, waiter_id: &str) -> Result<()> {
        let mut waiter = self
            .web_waiters
            .remove(waiter_id)
            .ok_or_else(|| errors::invalid("Unknown web command response"))?;
        self.finish_web_driving(&waiter)?;
        waiter.driving = false;
        if !waiter.permission.agent || !matches!(waiter.permission.session, SessionAccess::Probe) {
            self.web_waiters.insert(waiter_id.into(), waiter);
            return Err(errors::invalid("Unexpected web session probe response"));
        }
        let decision = self.ensure_web_permission(
            &waiter.permission,
            &waiter.params,
            PermissionRequestKind::WebSession,
        );
        let decision = match decision {
            Ok(decision) => decision,
            Err(error) => {
                self.web_waiters.insert(waiter_id.into(), waiter);
                return Err(error);
            }
        };
        match decision {
            PermissionDecision::Allowed => {
                waiter.permission.session = SessionAccess::Granted;
                self.resume_one_web_waiter(waiter_id, waiter)
            }
            PermissionDecision::Denied => {
                self.reject_removed_waiter(waiter, "Web session permission was denied")
            }
            PermissionDecision::Pending(request) => {
                waiter.host.clear();
                waiter.deadline_ms = request.expires_ms;
                waiter.permission.request_id = Some(request.request_id);
                waiter.permission.expires_ms = Some(request.expires_ms);
                self.web_waiters.insert(waiter_id.into(), waiter);
                Ok(())
            }
        }
    }

    pub(crate) fn web_permission_answer(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Value> {
        let params: PanePermissionAnswerParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid web permission answer"))?;
        let request = StorePort::web_permission_request(&self.store, &params.request_id)
            .map_err(errors::storage)?
            .filter(|request| request.pane_id == params.pane_id)
            .ok_or_else(|| errors::not_found("Web permission request not found"))?;
        if let Some(result) = request.result.clone() {
            return self.repeat_web_permission_answer(connection, params, result);
        }
        self.commit_web_permission_answer(connection, params, request)
    }

    fn repeat_web_permission_answer(
        &mut self,
        connection: &str,
        params: PanePermissionAnswerParams,
        result: PanePermissionAnswerResult,
    ) -> Result<Value> {
        let raw = serde_json::to_value(&params)
            .map_err(|_| errors::internal("Permission answer serialization failed"))?;
        Ok(self
            .mutate(connection, method::PANE_PERMISSION_ANSWER, &raw, |_| {
                Ok((serde_json::to_value(result)?, Vec::new()))
            })?
            .result)
    }

    fn commit_web_permission_answer(
        &mut self,
        connection: &str,
        params: PanePermissionAnswerParams,
        request: WebPermissionRequest,
    ) -> Result<Value> {
        let now = self.now_ms();
        let by = self.web_decision_actor(connection)?;
        let (result, allowed, grant) = permission_answer(&request, &params, now, &by)?;
        let saved = self.permission_pane_state(&request, allowed)?;
        let result_value = serde_json::to_value(&result)
            .map_err(|_| errors::internal("Permission result serialization failed"))?;
        let events = permission_events(&request, &saved, &result)?;
        let grant = grant.map(|scope| WebSessionGrant {
            workspace_id: request.workspace_id.clone(),
            domain: request.domain.clone(),
            scope,
            granted_by: decision_by_text(&by).into(),
            created_ms: now,
        });
        let raw = serde_json::to_value(&params)
            .map_err(|_| errors::internal("Permission answer serialization failed"))?;
        let request_id = request.request_id.clone();
        let mutation = self.mutate(connection, method::PANE_PERMISSION_ANSWER, &raw, |change| {
            if !change.answer_web_permission(&request_id, &result)? {
                return Err(soyeht_store::database::Error::InvalidWebPermission);
            }
            if let Some(grant) = &grant {
                change.save_web_session_grant(grant)?;
            }
            write_pane(change, &saved)?;
            Ok((result_value, events))
        })?;
        if !mutation.repeated {
            self.panes
                .get_mut(&request.pane_id)
                .ok_or_else(|| errors::internal("Web pane disappeared"))?
                .stored = saved;
            self.resume_web_permission(&request.request_id, allowed, &request.kind)?;
        }
        Ok(mutation.result)
    }

    fn permission_pane_state(
        &self,
        request: &WebPermissionRequest,
        allowed: bool,
    ) -> Result<crate::model::StoredPane> {
        let mut saved = self
            .panes
            .get(&request.pane_id)
            .ok_or_else(|| errors::not_found("Web pane not found"))?
            .stored
            .clone();
        if request.kind == PermissionRequestKind::WebSession {
            saved.pane.web_session = Some(if allowed {
                WebSessionState::Granted
            } else {
                WebSessionState::Anonymous
            });
        }
        Ok(saved)
    }

    fn web_decision_actor(&self, connection: &str) -> Result<DecisionBy> {
        match self
            .clients
            .get(connection)
            .and_then(|client| client.actor.as_ref())
            .map(|actor| &actor.role)
        {
            Some(Role::HumanLocal) => Ok(DecisionBy::Mac),
            Some(Role::HumanDevice) => Ok(DecisionBy::Iphone),
            _ => Err(errors::forbidden_role(
                "Only a person may answer web permissions",
            )),
        }
    }

    fn resume_web_permission(
        &mut self,
        request_id: &str,
        allowed: bool,
        kind: &PermissionRequestKind,
    ) -> Result<()> {
        let ids: Vec<_> = self
            .web_waiters
            .iter()
            .filter(|(_, waiter)| waiter.permission.request_id.as_deref() == Some(request_id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let mut waiter = self
                .web_waiters
                .remove(&id)
                .ok_or_else(|| errors::internal("Permission waiter disappeared"))?;
            if !allowed {
                self.reject_removed_waiter(waiter, "Web permission was denied")?;
                continue;
            }
            waiter.permission.request_id = None;
            waiter.permission.expires_ms = None;
            if *kind == PermissionRequestKind::WebSession {
                waiter.permission.session = SessionAccess::Granted;
            } else {
                waiter.permission.evaluation = true;
            }
            self.resume_one_web_waiter(&id, waiter)?;
        }
        Ok(())
    }

    fn resume_one_web_waiter(&mut self, id: &str, waiter: WebWaiter) -> Result<()> {
        self.web_waiters.insert(id.into(), waiter);
        if self.send_web_waiter(id).is_err() {
            self.fail_web_waiter(id, "Web host unavailable after permission")?;
        }
        Ok(())
    }

    fn reject_removed_waiter(&mut self, waiter: WebWaiter, message: &str) -> Result<()> {
        let error = errors::wire(errors::unauthorized(message));
        for (caller, request) in waiter.callers {
            if let Some(client) = self.clients.get(&caller) {
                client
                    .outbox
                    .send(crate::model::failure(request, error.clone())?);
            }
        }
        Ok(())
    }

    pub(crate) fn advance_web_permissions(&mut self) -> Result<()> {
        let expired = StorePort::expired_web_permissions(&self.store, self.now_ms())
            .map_err(errors::storage)?;
        for request in expired {
            self.expire_web_permission(request)?;
        }
        Ok(())
    }

    fn expire_web_permission(&mut self, request: WebPermissionRequest) -> Result<()> {
        let result = PanePermissionAnswerResult {
            request_id: request.request_id.clone(),
            result: PermissionResult::Expired,
            option_id: None,
            by: DecisionBy::System,
        };
        let saved = self.permission_pane_state(&request, false)?;
        let events = permission_events(&request, &saved, &result)?;
        let request_id = request.request_id.clone();
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine",
            &format!("web-permission-expire:{request_id}"),
            "web.permission.expire",
            |change| {
                change.answer_web_permission(&request_id, &result)?;
                write_pane(change, &saved)?;
                Ok((json!({}), events))
            },
        )
        .map_err(errors::storage)?;
        self.panes
            .get_mut(&request.pane_id)
            .ok_or_else(|| errors::internal("Web pane disappeared"))?
            .stored = saved;
        self.publish(&mutation.events)?;
        self.resume_web_permission(&request.request_id, false, &request.kind)
    }
}

fn session_access(agent: bool, incognito: bool, granted: bool) -> SessionAccess {
    if !agent {
        SessionAccess::Human
    } else if incognito {
        SessionAccess::Incognito
    } else if granted {
        SessionAccess::Granted
    } else {
        SessionAccess::Probe
    }
}

fn permission_decision(request: WebPermissionRequest) -> PermissionDecision {
    match request.result.as_ref() {
        None => PermissionDecision::Pending(Box::new(request)),
        Some(result)
            if result.result == PermissionResult::Selected
                && result.option_id.as_deref() != Some("deny") =>
        {
            PermissionDecision::Allowed
        }
        Some(_) => PermissionDecision::Denied,
    }
}

fn permission_events(
    request: &WebPermissionRequest,
    saved: &crate::model::StoredPane,
    result: &PanePermissionAnswerResult,
) -> Result<Vec<(String, Value)>> {
    let mut events = vec![model::permission_answered(&request.pane_id, result)?];
    if request.kind == PermissionRequestKind::WebSession {
        events.push(model::pane_updated(&saved.pane)?);
    }
    Ok(events)
}

fn permission_answer(
    request: &WebPermissionRequest,
    params: &PanePermissionAnswerParams,
    now: i64,
    by: &DecisionBy,
) -> Result<(
    PanePermissionAnswerResult,
    bool,
    Option<WebSessionGrantScope>,
)> {
    let expired = request.expires_ms <= now;
    let option = request
        .public
        .options
        .iter()
        .find(|option| option.option_id == params.option_id)
        .ok_or_else(|| errors::invalid("Unknown web permission option"))?;
    let allowed = !expired
        && matches!(
            option.kind,
            PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
        );
    let grant = session_grant(request, option, allowed)?;
    Ok((
        PanePermissionAnswerResult {
            request_id: request.request_id.clone(),
            result: if expired {
                PermissionResult::Expired
            } else {
                PermissionResult::Selected
            },
            option_id: (!expired).then(|| params.option_id.clone()),
            by: by.clone(),
        },
        allowed,
        grant,
    ))
}

fn session_grant(
    request: &WebPermissionRequest,
    option: &PermissionOption,
    allowed: bool,
) -> Result<Option<WebSessionGrantScope>> {
    if !allowed || request.kind != PermissionRequestKind::WebSession {
        return Ok(None);
    }
    match option.option_id.as_str() {
        "task" => request
            .task_id
            .clone()
            .map(WebSessionGrantScope::Task)
            .map(Some)
            .ok_or_else(|| errors::invalid("Web permission has no active task")),
        "workspace" => Ok(Some(WebSessionGrantScope::Workspace)),
        _ => Err(errors::invalid("Invalid web session permission option")),
    }
}

fn permission_title(kind: &PermissionRequestKind) -> &'static str {
    match kind {
        PermissionRequestKind::WebSession => "Use the signed-in web session?",
        PermissionRequestKind::WebEvaluate => "Run page code?",
        PermissionRequestKind::Agent => "Agent permission",
    }
}

fn permission_options(kind: &PermissionRequestKind, task: bool) -> Vec<PermissionOption> {
    let mut options = Vec::new();
    if *kind == PermissionRequestKind::WebSession {
        if task {
            options.push(permission_option(
                "task",
                PermissionOptionKind::AllowOnce,
                "This task only",
            ));
        }
        options.push(permission_option(
            "workspace",
            PermissionOptionKind::AllowAlways,
            "Always in this workspace",
        ));
    } else {
        options.push(permission_option(
            "allow",
            PermissionOptionKind::AllowOnce,
            "Allow once",
        ));
    }
    options.push(permission_option(
        "deny",
        PermissionOptionKind::RejectOnce,
        "Deny",
    ));
    options
}

fn permission_option(id: &str, kind: PermissionOptionKind, name: &str) -> PermissionOption {
    PermissionOption {
        option_id: id.into(),
        kind,
        name: name.into(),
    }
}

fn decision_by_text(by: &DecisionBy) -> &'static str {
    match by {
        DecisionBy::Mac => "mac",
        DecisionBy::Iphone => "iphone",
        DecisionBy::Rule => "rule",
        DecisionBy::System => "system",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dispatch::Outcome, model::RunningPane, outbox, ports::testing::FixedClock, screen::Screen,
    };
    use serde_json::json;
    use soyeht_protocol::{Actor, ErrorKind, WebCommandKind, frame::Frame};
    use std::{fs, path::PathBuf, sync::Arc};

    fn request(id: u64, method: &str, params: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
    }

    fn reply(core: &mut Core, connection: &str, request: Value) -> Value {
        let Outcome::Reply(value) = core.dispatch(connection, request).unwrap() else {
            panic!("expected reply")
        };
        value
    }

    async fn next_json(receiver: &mut outbox::Receiver) -> Value {
        let Frame::Json(bytes) = receiver.next().await.unwrap().unwrap().frame else {
            panic!("expected JSON frame")
        };
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn connect(core: &mut Core, host: bool) -> (String, outbox::Receiver) {
        let (sender, mut receiver) = outbox::channel();
        let connection = core.connect(true, false, sender).unwrap();
        let _ = next_json(&mut receiver).await;
        reply(
            core,
            &connection,
            request(
                1,
                "hello",
                json!({"client":"web-permission-test","clientVersion":"1",
                    "protocolVersion":1,"capabilities":if host { json!(["web_host"]) } else { json!([]) }}),
            ),
        );
        (connection, receiver)
    }

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("soyeht-web-permission-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    async fn setup() -> (
        Core,
        String,
        outbox::Receiver,
        String,
        outbox::Receiver,
        String,
        PathBuf,
    ) {
        let root = temp_root();
        let mut core = Core::open(&root).unwrap();
        core.clock = Arc::new(FixedClock(1_000));
        let (host, host_receiver) = connect(&mut core, true).await;
        let (caller, caller_receiver) = connect(&mut core, false).await;
        let workspace = reply(
            &mut core,
            &caller,
            request(
                2,
                "workspace.create",
                json!({"name":"Web","path":root,
                "idempotencyKey":"workspace"}),
            ),
        )["workspace"]["workspaceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let pane = reply(
            &mut core,
            &caller,
            request(
                3,
                "web.open",
                json!({"workspaceId":workspace,
                "url":"https://example.test/path","idempotencyKey":"open"}),
            ),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut agent = core.panes[&pane].stored.clone();
        agent.pane.pane_id = "agent-pane".into();
        agent.pane.mode = PaneMode::Agent;
        agent.pane.url = None;
        agent.pane.title = None;
        agent.pane.viewport = None;
        agent.pane.incognito = None;
        agent.pane.web_session = None;
        let agent_screen = Screen::open(&root.join("sessions/agent-pane"), false).unwrap();
        core.panes.insert(
            agent.pane.pane_id.clone(),
            RunningPane {
                stored: agent,
                screen: agent_screen,
            },
        );
        core.clients.get_mut(&caller).unwrap().actor = Some(Actor {
            role: Role::Agent,
            client: "bound-agent".into(),
            device_id: None,
            pane_id: Some("agent-pane".into()),
        });
        (
            core,
            host,
            host_receiver,
            caller,
            caller_receiver,
            pane,
            root,
        )
    }

    fn click(pane: &str, operation: &str) -> (Value, WebCommandParams) {
        let raw = json!({"paneId":pane,"ref":"node-1","idempotencyKey":operation});
        let params = WebCommandParams {
            pane_id: pane.into(),
            command: WebCommandKind::Click,
            args: json!({"ref":"node-1"}),
            timeout_ms: 30_000,
            operation_key: operation.into(),
        };
        (raw, params)
    }

    #[tokio::test]
    async fn agent_web_commands_publish_one_bounded_driving_interval() {
        let (mut core, host, mut host_rx, caller, mut caller_rx, pane, root) = setup().await;
        let (viewer, mut viewer_rx) = connect(&mut core, false).await;
        core.clients.get_mut(&viewer).unwrap().events = true;

        let (raw, params) = click(&pane, "driving");
        let id = core
            .queue_web_command(&caller, json!(9), "web.click", &raw, params)
            .unwrap();
        let started = next_json(&mut viewer_rx).await;
        assert_eq!(started["method"], "web.agent_driving");
        assert_eq!(started["params"]["paneId"], pane);
        assert_eq!(started["params"]["on"], true);
        assert_eq!(started["params"]["domain"], "example.test");
        let command = next_json(&mut host_rx).await;
        assert_eq!(command["id"], id);

        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":id,"result":{
                "paneId":pane,"cookiesPresent":false,"value":{"completed":true}}}),
        )
        .unwrap();
        let stopped = next_json(&mut viewer_rx).await;
        assert_eq!(stopped["method"], "web.agent_driving");
        assert_eq!(stopped["params"]["paneId"], pane);
        assert_eq!(stopped["params"]["on"], false);
        assert!(stopped["params"].get("domain").is_none());
        assert_eq!(next_json(&mut caller_rx).await["result"]["completed"], true);
        crate::test_paths::remove_after(core, root);
    }

    #[tokio::test]
    async fn signed_in_session_waits_for_a_person_and_workspace_grant_is_reused() {
        let (mut core, host, mut host_rx, caller, mut caller_rx, pane, root) = setup().await;
        let (raw, params) = click(&pane, "click-one");
        let id = core
            .queue_web_command(&caller, json!(10), "web.click", &raw, params)
            .unwrap();
        let probe = next_json(&mut host_rx).await;
        assert_eq!(
            probe["params"]["args"][INTERNAL_ACCESS_KEY]["session"],
            "probe"
        );
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":id,"result":{"paneId":pane,"cookiesPresent":true}}),
        )
        .unwrap();
        let pending = core
            .store
            .open_web_permission(
                &pane,
                &PermissionRequestKind::WebSession,
                "example.test",
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(pending.public.options.len(), 2);
        assert_eq!(
            core.panes[&pane].stored.pane.web_session,
            Some(WebSessionState::PermissionRequired)
        );
        let answer = reply(
            &mut core,
            &host,
            request(
                11,
                "pane.permission.answer",
                json!({"paneId":pane,
                "requestId":pending.request_id,"optionId":"workspace","idempotencyKey":"answer"}),
            ),
        );
        assert_eq!(answer["by"], "mac");
        let resumed = next_json(&mut host_rx).await;
        assert_eq!(
            resumed["params"]["args"][INTERNAL_ACCESS_KEY]["session"],
            "granted"
        );
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":resumed["id"],"result":{"paneId":pane,"value":{"completed":true}}}),
        )
        .unwrap();
        assert_eq!(next_json(&mut caller_rx).await["result"]["completed"], true);

        let (raw, params) = click(&pane, "click-two");
        core.queue_web_command(&caller, json!(12), "web.click", &raw, params)
            .unwrap();
        let granted = next_json(&mut host_rx).await;
        assert_eq!(
            granted["params"]["args"][INTERNAL_ACCESS_KEY]["session"],
            "granted"
        );
        crate::test_paths::remove_after(core, root);
    }

    #[tokio::test]
    async fn evaluation_waits_for_one_time_permission_and_reuses_its_receipt() {
        let (mut core, host, mut host_rx, caller, mut caller_rx, pane, root) = setup().await;
        let raw = json!({"paneId":pane,"js":"document.title","idempotencyKey":"eval-one"});
        assert!(matches!(
            core.dispatch(&caller, request(20, "web.evaluate", raw.clone())),
            Ok(Outcome::Queued)
        ));
        let pending = core
            .store
            .web_permission_for_operation(&pane, &PermissionRequestKind::WebEvaluate, "eval-one")
            .unwrap()
            .unwrap();
        assert_eq!(pending.public.options[0].option_id, "allow");
        let answer = reply(
            &mut core,
            &host,
            request(
                21,
                "pane.permission.answer",
                json!({"paneId":pane,
                "requestId":pending.request_id,"optionId":"allow","idempotencyKey":"allow"}),
            ),
        );
        assert_eq!(answer["optionId"], "allow");
        let allowed = next_json(&mut host_rx).await;
        assert_eq!(
            allowed["params"]["args"][INTERNAL_ACCESS_KEY]["evaluation"],
            true
        );
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":allowed["id"],"result":{
                "paneId":pane,"cookiesPresent":false,"value":{"value":"Example"}}}),
        )
        .unwrap();
        assert_eq!(
            next_json(&mut caller_rx).await["result"]["value"],
            "Example"
        );
        let repeated = core.dispatch(&caller, request(22, "web.evaluate", raw));
        let Outcome::Reply(repeated) = repeated.unwrap() else {
            panic!("evaluation retry should use the durable mutation receipt")
        };
        assert_eq!(repeated["value"], "Example");
        crate::test_paths::remove_after(core, root);
    }

    #[tokio::test]
    async fn evaluation_denial_is_durable_for_the_operation() {
        let (mut core, host, _host_rx, caller, mut caller_rx, pane, root) = setup().await;
        let raw = json!({"paneId":pane,"js":"document.title","idempotencyKey":"eval-deny"});
        assert!(matches!(
            core.dispatch(&caller, request(24, "web.evaluate", raw.clone())),
            Ok(Outcome::Queued)
        ));
        let pending = core
            .store
            .web_permission_for_operation(&pane, &PermissionRequestKind::WebEvaluate, "eval-deny")
            .unwrap()
            .unwrap();
        reply(
            &mut core,
            &host,
            request(
                25,
                "pane.permission.answer",
                json!({"paneId":pane,"requestId":pending.request_id,
                    "optionId":"deny","idempotencyKey":"deny"}),
            ),
        );
        assert_eq!(
            next_json(&mut caller_rx).await["error"]["data"]["kind"],
            "unauthorized"
        );
        let denied = core.queue_web_command(
            &caller,
            json!(26),
            "web.evaluate",
            &raw,
            WebCommandParams {
                pane_id: pane,
                command: WebCommandKind::Evaluate,
                args: json!({"js":"document.title"}),
                timeout_ms: 30_000,
                operation_key: "eval-deny".into(),
            },
        );
        assert!(denied.is_err_and(|error| errors::kind(&error) == ErrorKind::Unauthorized));
        crate::test_paths::remove_after(core, root);
    }

    #[tokio::test]
    async fn expired_permission_is_answered_by_the_system_and_fails_the_waiter() {
        let (mut core, host, mut host_rx, caller, mut caller_rx, pane, root) = setup().await;
        let (raw, params) = click(&pane, "expires");
        let id = core
            .queue_web_command(&caller, json!(30), "web.click", &raw, params)
            .unwrap();
        let _ = next_json(&mut host_rx).await;
        core.web_host_response(
            &host,
            json!({"jsonrpc":"2.0","id":id,"result":{"paneId":pane,"cookiesPresent":true}}),
        )
        .unwrap();
        core.clock = Arc::new(FixedClock(1_000 + QUESTION_TIMEOUT_MS));
        core.advance_web_permissions().unwrap();
        let expired = next_json(&mut caller_rx).await;
        assert_eq!(expired["error"]["data"]["kind"], "unauthorized");
        assert_eq!(
            core.panes[&pane].stored.pane.web_session,
            Some(WebSessionState::Anonymous)
        );
        assert!(
            core.store
                .expired_web_permissions(core.now_ms())
                .unwrap()
                .is_empty()
        );
        crate::test_paths::remove_after(core, root);
    }

    #[tokio::test]
    async fn durable_permission_answer_survives_a_host_disconnect() {
        let (mut core, host, _host_rx, caller, mut caller_rx, pane, root) = setup().await;
        let raw = json!({"paneId":pane,"js":"document.title","idempotencyKey":"host-gone"});
        assert!(matches!(
            core.dispatch(&caller, request(40, "web.evaluate", raw)),
            Ok(Outcome::Queued)
        ));
        let pending = core
            .store
            .web_permission_for_operation(&pane, &PermissionRequestKind::WebEvaluate, "host-gone")
            .unwrap()
            .unwrap();
        core.clients.get_mut(&host).unwrap().web_host = false;
        let answer = reply(
            &mut core,
            &host,
            request(
                41,
                "pane.permission.answer",
                json!({"paneId":pane,"requestId":pending.request_id,
                    "optionId":"allow","idempotencyKey":"host-gone-answer"}),
            ),
        );
        assert_eq!(answer["optionId"], "allow");
        assert_eq!(
            next_json(&mut caller_rx).await["error"]["data"]["kind"],
            "web_host_unavailable"
        );
        assert!(
            core.store
                .web_permission_for_operation(
                    &pane,
                    &PermissionRequestKind::WebEvaluate,
                    "host-gone",
                )
                .unwrap()
                .unwrap()
                .result
                .is_some()
        );
        crate::test_paths::remove_after(core, root);
    }

    #[tokio::test]
    async fn bound_agents_cannot_open_or_control_web_panes_in_another_workspace() {
        let (mut core, _host, _host_rx, caller, _caller_rx, pane, root) = setup().await;
        let open = core.dispatch(
            &caller,
            request(
                50,
                "web.open",
                json!({"workspaceId":"another-workspace","url":"example.test",
                    "idempotencyKey":"cross-open"}),
            ),
        );
        assert!(open.is_err_and(|error| errors::kind(&error) == ErrorKind::ForbiddenRole));

        core.panes.get_mut(&pane).unwrap().stored.pane.workspace_id = "another-workspace".into();
        let command = core.dispatch(
            &caller,
            request(
                51,
                "web.click",
                json!({"paneId":pane,"ref":"e1","idempotencyKey":"cross-click"}),
            ),
        );
        assert!(command.is_err_and(|error| errors::kind(&error) == ErrorKind::ForbiddenRole));
        crate::test_paths::remove_after(core, root);
    }
}

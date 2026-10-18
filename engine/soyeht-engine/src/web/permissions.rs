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
mod tests;

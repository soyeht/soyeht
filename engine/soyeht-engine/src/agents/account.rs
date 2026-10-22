//! What the person may do about an agent's sign-in at any time, not only when
//! the agent demands one: watch a sign-in run, cancel it, switch account and
//! sign out. The sign-in terminal itself lives in `authentication`.

use super::{
    Changes,
    authentication::auth_finished,
    harness_sign_in::SignInChange,
    purpose::Purpose,
    responses,
    state::{Phase, State},
};
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::{Value, json};
use soyeht_acp::session::AuthKind;
use soyeht_protocol::{AgentLoginPhase, AgentLoginProgress, AuthMethod, method};

/// The account fields `AgentSession` publishes.
pub(crate) struct AccountView {
    pub(crate) login: Option<AgentLoginProgress>,
    pub(crate) reauth_methods: Option<Vec<AuthMethod>>,
    pub(crate) can_sign_out: Option<bool>,
}

fn progress(phase: AgentLoginPhase, terminal: Option<&String>) -> AgentLoginProgress {
    AgentLoginProgress {
        phase,
        terminal_pane_id: terminal.cloned(),
    }
}

impl State {
    /// A session is open under a sign-in the agent accepted.
    pub(crate) fn is_signed_in(&self) -> bool {
        self.phase == Phase::Ready && !self.auth_required && self.session_id.is_some()
    }

    fn logout_pending(&self) -> bool {
        self.client
            .pending
            .values()
            .any(|call| Purpose::parse(&call.context) == Purpose::Logout)
    }

    pub(crate) fn account_view(&self) -> AccountView {
        let login = if self.logout_pending() {
            Some(progress(AgentLoginPhase::SigningOut, None))
        } else if let Some(terminal) = &self.unconfirmed_login_pane_id {
            Some(progress(AgentLoginPhase::Verifying, Some(terminal)))
        } else {
            self.login_pane_id
                .as_ref()
                .map(|terminal| progress(AgentLoginPhase::Waiting, Some(terminal)))
        };
        let controls = login.is_none() && self.is_signed_in();
        let reauth_methods = controls
            .then(|| self.terminal_methods())
            .filter(|methods| !methods.is_empty());
        let can_sign_out = (controls
            && self
                .initialized
                .as_ref()
                .is_some_and(|initialized| initialized.logout))
        .then_some(true);
        AccountView {
            login,
            reauth_methods,
            can_sign_out,
        }
    }

    fn terminal_methods(&self) -> Vec<AuthMethod> {
        self.initialized
            .iter()
            .flat_map(|initialized| &initialized.auth_methods)
            .filter(|method| method.kind == AuthKind::Terminal)
            .map(|method| method.public.clone())
            .collect()
    }

    /// Why the account of a working session cannot change right now.
    pub(crate) fn account_busy(&self) -> Result<()> {
        if self.login_pane_id.is_some()
            || self.unconfirmed_login_pane_id.is_some()
            || self.logout_pending()
        {
            return Err(errors::pane_busy("An account change is already running"));
        }
        if self.active.is_some() || self.operation.is_some() {
            return Err(errors::pane_busy(
                "Finish or cancel the current turn before changing the account",
            ));
        }
        Ok(())
    }
}

impl Core {
    /// Stops a running sign-in. The terminal closes with its process; the pane
    /// keeps the account it had, or keeps asking for one.
    pub fn agent_auth_cancel(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let id = errors::param_str(params, "paneId")?;
        let mut state = self.agent_state(id)?;
        let child_id = state
            .login_pane_id
            .take()
            .ok_or_else(|| errors::invalid("No sign-in is running for this pane"))?;
        state.relogin = false;
        state.last_update_ms = self.now_ms();
        let mut changes = Changes::default();
        changes
            .events
            .push(auth_finished(id, false, "Sign-in was cancelled.")?);
        if let Some(mut child) = self
            .panes
            .get(&child_id)
            .map(|pane| pane.stored.clone())
            .filter(|pane| !pane.closed)
        {
            changes.workspace_layout = self.prepare_pane_close(&mut child, &mut changes.events)?;
            changes.closed_login_pane = Some(child);
        }
        Ok(self
            .commit_agent(
                self.panes[id].stored.clone(),
                state,
                changes,
                Some((connection, method::PANE_AUTH_CANCEL, params)),
                json!({"paneId":id}),
            )?
            .result)
    }

    /// Asks the agent to forget its credentials. The agent restarts when it
    /// answers, finds no sign-in, and offers the sign-in methods again.
    pub fn agent_auth_logout(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let id = errors::param_str(params, "paneId")?;
        let mut state = self.agent_state(id)?;
        if !state.is_signed_in() {
            return Err(errors::invalid("This pane is not signed in"));
        }
        if !state
            .initialized
            .as_ref()
            .is_some_and(|initialized| initialized.logout)
        {
            return Err(errors::invalid("This agent cannot sign out from here"));
        }
        state.account_busy()?;
        let mut changes = Changes::default();
        responses::call(
            &mut state,
            &mut changes,
            "logout",
            json!({}),
            &Purpose::Logout,
            self.now_ms(),
        )?;
        Ok(self
            .commit_agent(
                self.panes[id].stored.clone(),
                state,
                changes,
                Some((connection, method::PANE_AUTH_LOGOUT, params)),
                json!({"paneId":id}),
            )?
            .result)
    }
}

/// The agent's answer to `logout`. Success restarts the agent so that it
/// reports the missing sign-in itself; a refusal leaves the session untouched.
pub(super) fn logout_response(
    state: &mut State,
    saved: &crate::model::StoredPane,
    changes: &mut Changes,
    ok: std::result::Result<(), String>,
) -> Result<()> {
    match ok {
        Ok(()) => {
            changes.sign_in = Some(SignInChange::forget(&state.launch.id));
            state.phase = Phase::RestartAfterProbe;
        }
        Err(reason) => changes.events.push(auth_finished(
            &saved.pane.pane_id,
            false,
            &format!("Signing out failed: {reason}"),
        )?),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::state::tests::state;
    use soyeht_acp::session::Initialization;

    fn state_with(authentication: serde_json::Value) -> State {
        let mut state = state();
        let mut initialize = json!({
            "protocolVersion": 1,
            "authMethods": [{"id": "login", "name": "Sign in", "type": "terminal"}]
        });
        initialize
            .as_object_mut()
            .unwrap()
            .extend(authentication.as_object().unwrap().clone());
        state.initialized = Some(Initialization::parse(&initialize).unwrap());
        state
    }

    #[test]
    fn signing_out_forgets_the_harness_sign_in_and_a_refusal_keeps_it() {
        let pane = crate::test_panes::pane("p", "w", soyeht_protocol::PaneMode::Agent, "/tmp");
        let stored = crate::model::StoredPane {
            pane,
            closed: false,
            instance_key: "key".into(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        };
        let mut state = state_with(json!({}));
        let mut refused = Changes::default();
        logout_response(&mut state, &stored, &mut refused, Err("no".into())).unwrap();
        assert_eq!(refused.sign_in, None);
        let mut signed_out = Changes::default();
        logout_response(&mut state, &stored, &mut signed_out, Ok(())).unwrap();
        assert_eq!(
            signed_out.sign_in,
            Some(SignInChange::forget(&state.launch.id))
        );
    }

    #[test]
    fn public_state_keeps_authentication_choices_across_snapshot_reconnection() {
        let mut state = state_with(json!({}));
        state.phase = Phase::NeedsLogin;
        let public = state.public(0);
        assert_eq!(public.auth_methods.len(), 1);
        assert_eq!(public.auth_methods[0].id, "login");
        assert!(public.auth_methods[0].terminal);
    }

    #[test]
    fn public_state_hides_advertised_authentication_until_the_agent_requires_it() {
        let mut state = state_with(json!({}));
        state.phase = Phase::Ready;
        assert!(state.public(0).auth_methods.is_empty());
    }

    #[test]
    fn a_signed_in_session_offers_the_account_actions_the_agent_supports() {
        let mut state = state_with(json!({}));
        state.phase = Phase::Ready;
        state.session_id = Some("session".into());
        let public = state.public(0);
        assert!(public.auth_methods.is_empty());
        assert_eq!(public.reauth_methods.unwrap()[0].id, "login");
        assert_eq!(public.can_sign_out, None);

        let mut state = state_with(json!({"agentCapabilities": {"auth": {"logout": {}}}}));
        state.phase = Phase::Ready;
        state.session_id = Some("session".into());
        assert_eq!(state.public(0).can_sign_out, Some(true));
    }

    #[test]
    fn account_actions_are_hidden_without_a_session_and_while_one_runs() {
        let mut state = state_with(json!({"agentCapabilities": {"auth": {"logout": {}}}}));
        state.phase = Phase::Ready;
        assert!(state.public(0).reauth_methods.is_none(), "no session yet");

        state.session_id = Some("session".into());
        state.login_pane_id = Some("terminal".into());
        let public = state.public(0);
        let login = public.login.unwrap();
        assert_eq!(login.phase, AgentLoginPhase::Waiting);
        assert_eq!(login.terminal_pane_id.as_deref(), Some("terminal"));
        assert!(public.reauth_methods.is_none());
        assert_eq!(public.can_sign_out, None);

        state.login_pane_id = None;
        state.unconfirmed_login_pane_id = Some("terminal".into());
        assert_eq!(
            state.public(0).login.unwrap().phase,
            AgentLoginPhase::Verifying
        );
    }

    #[test]
    fn the_account_cannot_change_during_a_turn_or_another_change() {
        let mut state = state_with(json!({}));
        state.phase = Phase::Ready;
        state.session_id = Some("session".into());
        assert!(state.account_busy().is_ok());
        state.login_pane_id = Some("terminal".into());
        assert!(state.account_busy().is_err());
    }
}

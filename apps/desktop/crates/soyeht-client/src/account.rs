//! What a signed-in agent pane offers about its account: the account section under the harness
//! switch. The engine owns the sign-in (its card is `cards::sign_in`); this only names what it
//! reports, so every client says the same thing (the Mac's `AgentAccountActions`, which passes the
//! same vectors).

use crate::cards::SignInMethod;
use soyeht_protocol::{AgentSession, PaneState};

/// One thing a person can do about the account of a signed-in agent pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountAction {
    SwitchTo(String),
    SignOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountItem {
    pub action: AccountAction,
    pub title: String,
}

impl AccountItem {
    pub fn is_destructive(&self) -> bool {
        self.action == AccountAction::SignOut
    }
}

/// What a signed-in agent pane offers about its account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountActions {
    pub switch_methods: Vec<SignInMethod>,
    pub can_sign_out: bool,
    /// The engine refuses an account change while a turn runs or another change is in flight.
    pub enabled: bool,
}

pub const SECTION_TITLE: &str = "Account";
pub const SWITCH_TITLE: &str = "Switch account";
pub const SIGN_OUT_TITLE: &str = "Sign out";
pub const EXPLANATION: &str =
    "Switching signs in again in a terminal pane. The conversation and its history stay.";

impl AccountActions {
    /// `None` when the agent offers nothing, for example while it still needs its first sign-in.
    pub fn new(
        agent: Option<&AgentSession>,
        state: Option<&PaneState>,
        connected: bool,
    ) -> Option<Self> {
        let switch_methods: Vec<SignInMethod> = agent
            .and_then(|agent| agent.reauth_methods.as_deref())
            .unwrap_or_default()
            .iter()
            .map(SignInMethod::from)
            .collect();
        let can_sign_out = agent.is_some_and(|agent| agent.can_sign_out == Some(true));
        if switch_methods.is_empty() && !can_sign_out {
            return None;
        }
        let turn_running = state.is_some_and(crate::agent::is_running);
        Some(Self {
            switch_methods,
            can_sign_out,
            enabled: connected
                && agent.is_some_and(|agent| agent.operation.is_none())
                && !turn_running,
        })
    }

    /// The one list of account actions and their labels.
    pub fn items(&self) -> Vec<AccountItem> {
        let several = self.switch_methods.len() > 1;
        let mut items: Vec<AccountItem> = self
            .switch_methods
            .iter()
            .map(|method| AccountItem {
                action: AccountAction::SwitchTo(method.id.clone()),
                title: if several {
                    format!("{SWITCH_TITLE}: {}", method.name)
                } else {
                    SWITCH_TITLE.to_owned()
                },
            })
            .collect();
        if self.can_sign_out {
            items.push(AccountItem {
                action: AccountAction::SignOut,
                title: SIGN_OUT_TITLE.to_owned(),
            });
        }
        items
    }
}

#[cfg(test)]
mod tests;

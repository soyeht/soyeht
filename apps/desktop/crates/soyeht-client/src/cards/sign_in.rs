//! The sign-in card of an agent pane: the choices when the agent needs a sign-in, the progress of
//! one that runs, and the reason one did not finish. The engine owns every state; this only says
//! what it reports, so every client says the same thing.

use soyeht_protocol::{AgentLoginPhase, AgentSession, AuthMethod};

pub const CHOOSE_TERMINAL_HINT: &str =
    "Choose a method to open its sign-in terminal. Enter credentials there.";
pub const SHOW_TERMINAL: &str = "Show terminal";
pub const CANCEL_SIGN_IN: &str = "Cancel sign-in";
pub const DISMISS: &str = "Dismiss";

/// A way to sign in, as the card lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInMethod {
    pub id: String,
    pub name: String,
    pub detail: Option<String>,
    /// The method runs in a terminal pane next to the agent.
    pub opens_terminal: bool,
}

impl From<&AuthMethod> for SignInMethod {
    fn from(method: &AuthMethod) -> Self {
        Self {
            id: method.id.clone(),
            name: method.name.clone(),
            detail: method.description.clone(),
            opens_terminal: method.terminal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInState {
    /// Nothing to sign in to and nothing running.
    Hidden,
    /// The agent needs a sign-in. `failure` says why the last attempt did not finish.
    Choose {
        methods: Vec<SignInMethod>,
        failure: Option<String>,
    },
    /// A sign-in runs in a terminal pane; the person finishes it in the browser.
    Waiting {
        terminal_pane_id: Option<String>,
    },
    /// The sign-in ended and the agent restarts to check the new account.
    Verifying,
    SigningOut,
    /// A sign-in did not finish on a pane that keeps its previous account.
    Failed {
        reason: String,
    },
}

impl SignInState {
    pub fn new(
        agent: Option<&AgentSession>,
        auth_methods: &[AuthMethod],
        failure: Option<&str>,
    ) -> Self {
        match agent.and_then(|agent| agent.login.as_ref()) {
            Some(login) => match login.phase {
                AgentLoginPhase::SigningOut => Self::SigningOut,
                AgentLoginPhase::Verifying => Self::Verifying,
                AgentLoginPhase::Waiting => Self::Waiting {
                    terminal_pane_id: login.terminal_pane_id.clone(),
                },
            },
            None if !auth_methods.is_empty() => Self::Choose {
                methods: auth_methods.iter().map(SignInMethod::from).collect(),
                failure: failure.map(str::to_owned),
            },
            None => failure.map_or(Self::Hidden, |reason| Self::Failed {
                reason: reason.to_owned(),
            }),
        }
    }

    pub fn title(&self) -> Option<&'static str> {
        match self {
            Self::Hidden => None,
            Self::Choose { .. } => Some("Sign in to continue"),
            Self::Waiting { .. } => Some("Finish signing in"),
            Self::Verifying => Some("Checking the new account"),
            Self::SigningOut => Some("Signing out"),
            Self::Failed { .. } => Some("Sign-in did not finish"),
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Hidden | Self::Choose { .. } => None,
            Self::Waiting { .. } => Some(
                "Complete the sign-in in your browser. Soyeht continues as soon as it is done.",
            ),
            Self::Verifying => Some("Restarting the agent. The conversation stays."),
            Self::SigningOut => Some("The agent is forgetting its account."),
            Self::Failed { reason } => Some(reason),
        }
    }

    /// The state is moving on its own, so the card shows progress rather than choices.
    pub fn is_working(&self) -> bool {
        matches!(self, Self::Verifying | Self::SigningOut)
    }
}

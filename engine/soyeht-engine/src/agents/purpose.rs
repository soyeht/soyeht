//! Why the engine sent a request to an agent. The pending request keeps the
//! purpose as the string `soyeht-acp` stores with it, so persisted checkpoints
//! stay readable; everything in the engine names and matches the type instead.

const INITIALIZE: &str = "initialize";
const OPEN: &str = "open";
const LOAD: &str = "load";
const AUTHENTICATE: &str = "auth";
const LOGOUT: &str = "logout";
const PROBE_PREFIX: &str = "probe:";

/// A step of the capability probe that runs once per agent version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeStep {
    New,
    List,
    Load,
    Config,
}

impl ProbeStep {
    const ALL: [Self; 4] = [Self::New, Self::List, Self::Load, Self::Config];

    fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::List => "list",
            Self::Load => "load",
            Self::Config => "config",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// The handshake that learns the agent's capabilities and sign-in methods.
    Initialize,
    /// `session/new` for the pane's own session.
    Open,
    /// `session/load` of the pane's existing session.
    Load,
    /// `authenticate` through the protocol.
    Authenticate,
    /// `logout`: the agent forgets its account.
    Logout,
    Probe(ProbeStep),
    /// A request named by an id: a prompt turn or a configuration operation.
    Identified(String),
}

impl Purpose {
    /// The string stored with the pending request.
    pub fn context(&self) -> String {
        match self {
            Self::Initialize => INITIALIZE.into(),
            Self::Open => OPEN.into(),
            Self::Load => LOAD.into(),
            Self::Authenticate => AUTHENTICATE.into(),
            Self::Logout => LOGOUT.into(),
            Self::Probe(step) => format!("{PROBE_PREFIX}{}", step.name()),
            Self::Identified(id) => id.clone(),
        }
    }

    pub fn parse(context: &str) -> Self {
        match context {
            INITIALIZE => Self::Initialize,
            OPEN => Self::Open,
            LOAD => Self::Load,
            AUTHENTICATE => Self::Authenticate,
            LOGOUT => Self::Logout,
            _ => context
                .strip_prefix(PROBE_PREFIX)
                .and_then(|name| ProbeStep::ALL.into_iter().find(|step| step.name() == name))
                .map_or_else(|| Self::Identified(context.into()), Self::Probe),
        }
    }

    pub fn identified(id: &str) -> Self {
        Self::Identified(id.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_purpose_round_trips_through_its_stored_context() {
        let purposes = [
            Purpose::Initialize,
            Purpose::Open,
            Purpose::Load,
            Purpose::Authenticate,
            Purpose::Logout,
            Purpose::Probe(ProbeStep::New),
            Purpose::Probe(ProbeStep::List),
            Purpose::Probe(ProbeStep::Load),
            Purpose::Probe(ProbeStep::Config),
            Purpose::identified("prompt-1"),
        ];
        for purpose in purposes {
            assert_eq!(Purpose::parse(&purpose.context()), purpose);
        }
    }

    #[test]
    fn stored_contexts_keep_the_strings_checkpoints_were_written_with() {
        assert_eq!(Purpose::Authenticate.context(), "auth");
        assert_eq!(Purpose::Probe(ProbeStep::New).context(), "probe:new");
        assert_eq!(
            Purpose::parse("probe:config"),
            Purpose::Probe(ProbeStep::Config)
        );
        // An unknown probe step is an id, never a probe.
        assert_eq!(
            Purpose::parse("probe:other"),
            Purpose::identified("probe:other")
        );
    }
}

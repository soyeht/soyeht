//! The permission mode an agent that started a pane puts on it.
//!
//! A pane started by an agent (a task executor a chief dispatched, a reviewer of an executor)
//! may run in no mode above the mode of the agent that started it. The pane keeps the ceiling
//! in its state, and every time a session of the pane opens, the mode the harness actually
//! runs is checked against it: a harness drops a mode it does not offer and starts in its own,
//! which can rank higher than the one asked for.

use super::state::State;
use serde::{Deserialize, Serialize};
use soyeht_acp::config::{FULL_ACCESS_KIND, Options, mode_kind_rank};

/// The highest rank a mode can have: full access, which only a person gives.
fn highest_rank() -> usize {
    mode_kind_rank(Some(FULL_ACCESS_KIND)).unwrap_or(usize::MAX)
}

/// The kind a harness announces for one of its modes.
fn announced_kind<'a>(options: &'a Options, mode: &str) -> Option<&'a str> {
    options
        .mode()?
        .options
        .iter()
        .find(|choice| choice.value == mode)?
        .kind
        .as_deref()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ModeCeiling {
    /// The rank of the starting agent's mode; a mode that cannot be ranked counts as the lowest.
    rank: usize,
    /// The harness and the mode of the starting agent: at its own rank, a pane may run only the
    /// very same mode on the very same harness, since a rank groups modes that differ.
    harness: String,
    mode: Option<String>,
    /// Whether any mode at the starting agent's rank is allowed. A reviewer reads what its
    /// executor wrote, so it may run at the executor's rank, never above it.
    same_rank_ok: bool,
    /// The mode the session runs has been checked since the session opened.
    #[serde(default)]
    checked: bool,
    /// A lower mode was already asked for in this session.
    #[serde(default)]
    corrected: bool,
}

impl ModeCeiling {
    /// The ceiling the mode `state` is in puts on a pane it starts.
    pub(crate) fn of(state: &State, same_rank_ok: bool) -> Self {
        let current = state.options.current_mode();
        Self {
            rank: current
                .and_then(|mode| mode_kind_rank(announced_kind(&state.options, mode)))
                .unwrap_or(0),
            harness: state.launch.id.clone(),
            mode: current.map(str::to_owned),
            same_rank_ok,
            checked: false,
            corrected: false,
        }
    }

    /// The ceiling of an agent the engine does not know: the lowest, with no mode of its own.
    pub(crate) fn unknown() -> Self {
        Self {
            rank: 0,
            harness: String::new(),
            mode: None,
            same_rank_ok: false,
            checked: false,
            corrected: false,
        }
    }

    /// Whether the mode `mode` of `harness`, which the harness announces with `kind`, stays
    /// within the ceiling: below its rank, or the very same mode on the very same harness.
    /// Full access is never within it, and a mode with no rank only for a ceiling that is
    /// full access already.
    pub(crate) fn allows(&self, harness: &str, mode: &str, kind: Option<&str>) -> bool {
        let Some(rank) = mode_kind_rank(kind) else {
            return self.rank >= highest_rank();
        };
        if rank >= highest_rank() {
            return false;
        }
        rank < self.rank
            || (rank == self.rank
                && (self.same_rank_ok
                    || (harness == self.harness && Some(mode) == self.mode.as_deref())))
    }

    /// The most permissive mode `options` announces that stays within the ceiling.
    fn best(&self, harness: &str, options: &Options) -> Option<String> {
        let choices = &options.mode()?.options;
        choices
            .iter()
            .filter(|choice| self.allows(harness, &choice.value, choice.kind.as_deref()))
            .rev()
            .max_by_key(|choice| mode_kind_rank(choice.kind.as_deref()))
            .map(|choice| choice.value.clone())
    }

    /// A session opened: the mode it runs is checked again.
    pub(crate) fn reopened(&mut self) {
        self.checked = false;
        self.corrected = false;
    }

    pub(crate) fn pending(&self) -> bool {
        !self.checked
    }

    /// Whether the mode the session runs, or is about to run (`wanted`, when the harness offers
    /// it), is within the ceiling; else what to do about it.
    pub(crate) fn verdict(&mut self, state: &State, wanted: Option<&str>) -> Verdict {
        let harness = state.launch.id.as_str();
        let options = &state.options;
        let running = wanted
            .filter(|mode| options.offers_mode(mode))
            .or_else(|| options.current_mode());
        let within =
            running.is_some_and(|mode| self.allows(harness, mode, announced_kind(options, mode)));
        if within {
            self.checked = wanted.is_none();
            return Verdict::Within;
        }
        match self.best(harness, options) {
            Some(mode) if !self.corrected => {
                self.corrected = true;
                Verdict::Switch(mode)
            }
            _ => Verdict::Stop,
        }
    }
}

/// What a pane with a ceiling does about the mode its session runs.
#[derive(Debug, PartialEq)]
pub(crate) enum Verdict {
    Within,
    /// Ask the harness for this mode instead.
    Switch(String),
    /// No mode of the harness stays within the ceiling.
    Stop,
}

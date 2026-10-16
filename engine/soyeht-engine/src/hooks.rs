//! The seam through which a test makes the running engine misbehave on demand.
//!
//! A production engine runs with [`NoHooks`]: every question answers "no" and nothing is
//! read from the data directory or the environment. The hooks that act live in
//! [`crate::test_hooks`], which exists only when the `test-hooks` feature is on, and
//! `soyeht-engine-test` is the only program that installs them (ADR 0091).
//!
//! Hooks run on the engine's own paths, some of them under the Core lock, and a hook may read
//! a marker file or close a process there. That is acceptable only because a hook exists solely
//! in the test program: the rule against blocking IO under the Core lock (ADR 0079) is for
//! code a person's engine runs.

use crate::{core::Core, errors::Result, model::RunningPane};
use soyeht_usage::host::Host;
use std::sync::Arc;

pub trait Hooks: Send + Sync {
    /// Runs before an agent pane is polled; an error fails that poll.
    fn before_agent_poll(&self, _core: &mut Core, _pane: &str) -> Result<()> {
        Ok(())
    }

    /// Runs on the candidate pane just before a harness switch commits.
    fn before_switch_commit(&self, _pane: &mut RunningPane) -> Result<()> {
        Ok(())
    }

    /// Whether closing the process a harness switch retired fails.
    fn fail_old_close(&self) -> bool {
        false
    }

    /// Whether the editor holds its autosaves.
    fn pause_editor_autosave(&self) -> bool {
        false
    }

    /// Whether skill folders are read on the probe instead of by file events.
    fn poll_skills_only(&self) -> bool {
        false
    }

    /// A host that replaces the machine's files, commands and network for the usage sources.
    /// The background usage schedule does not run over it: the test drives refreshes itself.
    fn usage_host(&self) -> Option<Arc<dyn Host>> {
        None
    }
}

/// The hooks of every shipped engine: none.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoHooks;

impl Hooks for NoHooks {}

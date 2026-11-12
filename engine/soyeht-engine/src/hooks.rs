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

use crate::{
    core::Core, editor::FileSystemPort, errors::Result, model::RunningPane, ports::PortScanPort,
};
use soyeht_ink::Recognizer;
use soyeht_usage::host::Host;
use std::sync::Arc;

pub trait Hooks: Send + Sync {
    /// Ends the process with `code`. The engine calls it when it cannot go on, for example on a
    /// poisoned Core lock (ADR 0159), so that the service manager starts a new one. A test
    /// overrides it to record the call; the default is the only place the engine exits from.
    fn end_process(&self, code: i32) {
        std::process::exit(code)
    }

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

    /// The longest the tick sleeps with no deadline and no event (ADR 0147, step 1b); `None` is
    /// the default of the shipped engine. A test raises it to show that no subsystem of the tick
    /// relies on it.
    fn tick_ceiling(&self) -> Option<std::time::Duration> {
        None
    }

    /// A scan of the listening ports that replaces the system's. A test that follows the deadlines
    /// of the tick uses one that cannot list, so the scan every five seconds is not one of them.
    fn port_scan(&self) -> Option<Box<dyn PortScanPort>> {
        None
    }

    /// Whether the editor holds its autosaves.
    fn pause_editor_autosave(&self) -> bool {
        false
    }

    /// Whether the user's skill folder is followed from the start. A test that follows the
    /// deadlines of the tick leaves it out, so its probe every two seconds is not one of them.
    fn follows_user_skills(&self) -> bool {
        true
    }

    /// Whether skill folders are read on the probe instead of by file events.
    fn poll_skills_only(&self) -> bool {
        false
    }

    /// A disk that replaces the machine's for the files the editor opens, saves and watches,
    /// for example a slow one.
    fn editor_disk(&self) -> Option<Box<dyn FileSystemPort>> {
        None
    }

    /// Whether every local client counts as the signed Mac app, which a test program is not.
    fn attests_local_ui(&self) -> bool {
        false
    }

    /// A recognizer that replaces the system's for `ink.recognize`.
    fn ink_recognizer(&self) -> Option<Box<dyn Recognizer>> {
        None
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

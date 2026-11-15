//! Which harness a terminal pane runs (ADR 0165, slice 2).
//!
//! A terminal pane has a harness when the foreground program of its terminal is one the engine
//! knows: Claude Code, Codex, OpenCode or Cursor Agent. The supervisor reports the programs
//! (`Foreground`), [`harnesses`] tells which is a harness, and [`version`] reads its version.
//! The result is `Pane.terminalHarness`, which `agent.list` also reads to list the pane.
//!
//! Nothing here holds the Core lock across the supervisor or a command (ADR 0079). A scan is a
//! background job that asks the supervisor and runs `--version` for a file it has not seen; its
//! result is applied under the lock. It runs on a slow period while an app watches, and on demand
//! for `agent.list`, whose reply waits for a scan only when the last one is older than
//! [`FRESH_MS`].

mod harnesses;
mod version;

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    offload::Offload,
    supervisor_handle::Supervisor,
    tick_due::Next,
};
use soyeht_protocol::{Pane, PaneMode, PaneState, TerminalHarness};
use soyeht_ptyd::wire::{FEATURE_FOREGROUND, MAX_FOREGROUND, Program, Request, Response};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use version::Versions;

/// The time between scans while an app watches.
const INTERVAL_MS: i64 = 2_000;
/// How recent a scan is good enough to answer `agent.list` with no new one.
const FRESH_MS: i64 = 1_000;
/// How long a request waits for a scan: the supervisor's answer, and `--version` for each file
/// not seen before (each has its own deadline, and a harness is started rarely).
const SCAN_TIMEOUT: Duration = Duration::from_secs(20);
/// The background job of the periodic scan.
const SCAN_JOB: &str = "terminal.foreground";

/// What the tick remembers of the scans.
#[derive(Default)]
pub(crate) struct Service {
    /// When the next periodic scan is due, in the engine's clock.
    next_ms: i64,
    /// When the last scan was applied, and the workspace it was limited to (`None`: every one).
    scanned: Option<(Option<String>, i64)>,
    /// Whether the last look found a terminal to scan and an app that watches.
    watching: bool,
    versions: Arc<Mutex<Versions>>,
}

/// A terminal pane to ask the supervisor about.
struct Target {
    pane_id: String,
    instance_id: String,
}

/// What a scan asks, with the supervisor link it asks on.
struct Scan {
    supervisor: Supervisor,
    targets: Vec<Target>,
    versions: Arc<Mutex<Versions>>,
}

/// What the supervisor and the table said of one pane.
struct Found {
    pane_id: String,
    instance_id: String,
    harness: Option<TerminalHarness>,
}

impl Scan {
    /// Asks the supervisor and reads the version of each harness it finds. It runs with no lock.
    /// A pane the supervisor does not answer for keeps what it had: the supervisor could not read
    /// it, which does not say the harness left.
    fn run(self) -> Result<Vec<Found>> {
        let instance_ids = self
            .targets
            .iter()
            .map(|target| target.instance_id.clone())
            .collect();
        let Response::Foregrounds(answers) =
            self.supervisor.call(Request::Foreground { instance_ids })?
        else {
            return Err(errors::ptyd_unavailable("Invalid foreground response"));
        };
        Ok(self
            .targets
            .iter()
            .filter_map(|target| {
                let answer = answers
                    .iter()
                    .find(|answer| answer.instance_id == target.instance_id)?;
                Some(Found {
                    pane_id: target.pane_id.clone(),
                    instance_id: target.instance_id.clone(),
                    harness: harness_of(&answer.programs, &self.versions),
                })
            })
            .collect())
    }
}

/// The harness the programs of a terminal hold, with its version, if they hold one.
fn harness_of(programs: &[Program], versions: &Mutex<Versions>) -> Option<TerminalHarness> {
    let matched = harnesses::pick(programs)?;
    let version =
        version::question(&matched).and_then(|question| version::read(versions, &question));
    Some(TerminalHarness {
        id: matched.known.id.into(),
        name: matched.known.name.into(),
        version,
    })
}

impl Core {
    /// The terminal panes a scan asks the supervisor about, and the panes that keep a harness
    /// after their process ended, which are cleared with no question. `workspace` limits the
    /// scan to one workspace.
    fn foreground_targets(&mut self, workspace: Option<&str>) -> Result<Vec<Target>> {
        let mut targets = Vec::new();
        let mut gone = Vec::new();
        for (id, running) in &self.panes {
            let stored = &running.stored;
            if stored.closed
                || stored.pane.mode != PaneMode::Terminal
                || self.agents.has(id)
                || workspace.is_some_and(|workspace| stored.pane.workspace_id != workspace)
            {
                continue;
            }
            match &stored.instance_id {
                Some(instance_id) if stored.pane.state != PaneState::Dead => {
                    if targets.len() < MAX_FOREGROUND {
                        targets.push(Target {
                            pane_id: id.clone(),
                            instance_id: instance_id.clone(),
                        });
                    }
                }
                _ if stored.pane.terminal_harness.is_some() => gone.push(id.clone()),
                _ => {}
            }
        }
        // A process that ended, or a pane restored with the harness of its last run, is a plain
        // terminal until a scan says otherwise.
        for id in gone {
            self.set_terminal_harness(&id, None)?;
        }
        Ok(targets)
    }

    /// A scan of `targets`, or `None` when the supervisor that is running does not list the
    /// `foreground` feature (it was started before this engine): the terminals then have no
    /// harness, and the engine asks nothing it cannot answer.
    fn start_scan(&mut self, targets: Vec<Target>) -> Result<Option<Scan>> {
        let supervisor = self.supervisor()?;
        if !supervisor.has_feature(FEATURE_FOREGROUND) {
            return Ok(None);
        }
        Ok(Some(Scan {
            supervisor,
            targets,
            versions: Arc::clone(&self.foreground().versions),
        }))
    }

    /// The tick's scan: starts one when an app watches, a terminal is open and the period is up.
    pub(crate) fn advance_foreground(&mut self) -> Result<()> {
        let now = self.now_ms();
        if now < self.foreground().next_ms || self.background.is_active(SCAN_JOB) {
            return Ok(());
        }
        let watched = self.clients.values().any(|client| client.events);
        let targets = if watched {
            self.foreground_targets(None)?
        } else {
            Vec::new()
        };
        self.foreground_mut().watching = !targets.is_empty();
        self.foreground_mut().next_ms = now + INTERVAL_MS;
        if targets.is_empty() {
            return Ok(());
        }
        let Some(scan) = self.start_scan(targets)? else {
            self.foreground_mut().watching = false;
            return Ok(());
        };
        let used = scan.supervisor.clone();
        self.spawn_background(
            SCAN_JOB.into(),
            move || scan.run(),
            move |core, scanned| core.finish_foreground(&used, None, scanned),
        )
    }

    /// When the tick must run for the period, while there is something to scan.
    pub(crate) fn foreground_due(&self, next: &mut Next, now: i64) {
        if self.foreground().watching {
            next.at_ms(now, self.foreground().next_ms);
        }
    }

    /// Puts a scan's findings into the panes, for each that is still the process it read. A scan
    /// that failed on a link that was replaced meanwhile says nothing about the new one.
    fn finish_foreground(
        &mut self,
        used: &Supervisor,
        workspace: Option<String>,
        scanned: Result<Vec<Found>>,
    ) -> Result<()> {
        let Some(found) = self.settle_if_current(used, scanned)? else {
            return Ok(());
        };
        self.foreground_mut().scanned = Some((workspace, self.now_ms()));
        for Found {
            pane_id,
            instance_id,
            harness,
        } in found
        {
            let current = self.panes.get(&pane_id).is_some_and(|running| {
                !running.stored.closed
                    && running.stored.instance_id.as_deref() == Some(instance_id.as_str())
            });
            if current {
                self.set_terminal_harness(&pane_id, harness)?;
            }
        }
        Ok(())
    }

    /// Records the harness of a pane, and tells the apps when it changed.
    fn set_terminal_harness(&mut self, id: &str, harness: Option<TerminalHarness>) -> Result<()> {
        let mut saved = self.pane(id)?.stored.clone();
        if saved.pane.terminal_harness == harness {
            return Ok(());
        }
        saved.pane.terminal_harness = harness;
        self.save_pane(id, saved, "terminal-harness", Vec::new())
    }

    /// `agent.list`: the agent panes of the sender's workspace, and its terminal panes that run a
    /// known harness. The terminals are as fresh as [`FRESH_MS`], by a scan the reply waits for.
    pub(crate) fn agent_list(&mut self, sender: Pane) -> Result<Outcome> {
        let workspace_id = sender.workspace_id.clone();
        let fresh = self
            .foreground()
            .scanned
            .as_ref()
            .is_some_and(|(scope, at)| {
                scope.as_deref().is_none_or(|scope| scope == workspace_id)
                    && self.now_ms().saturating_sub(*at) < FRESH_MS
            });
        let targets = if fresh {
            Vec::new()
        } else {
            self.foreground_targets(Some(&workspace_id))?
        };
        if targets.is_empty() {
            return Ok(Outcome::Reply(self.peer_listing(&sender)));
        }
        let Some(scan) = self.start_scan(targets)? else {
            return Ok(Outcome::Reply(self.peer_listing(&sender)));
        };
        let used = scan.supervisor.clone();
        Ok(Outcome::Offload(
            Offload::new(
                "Terminal scan timed out",
                SCAN_TIMEOUT,
                move || scan.run(),
                move |core, scanned| {
                    core.finish_foreground(&used, Some(workspace_id), scanned)?;
                    Ok(Outcome::Reply(core.peer_listing(&sender)))
                },
            )
            .concurrent(),
        ))
    }
}

#[cfg(test)]
mod tests;

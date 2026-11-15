//! Which harness a terminal pane runs (ADR 0165, slice 2).
//!
//! A terminal pane has a harness when the foreground program of its terminal is one the engine
//! knows: Claude Code, Codex, OpenCode or Cursor Agent. The supervisor reports the programs
//! (`Foreground`), [`harnesses`] tells which is a harness, and the install path says its version
//! (no program is ever run to ask). The result is `Pane.terminalHarness`, which `agent.list` also
//! reads to list the pane.
//!
//! **When a pane is read.** Never on a period, for every live terminal. A pane is read on demand
//! (`agent.list` reads them all unless the last read is younger than [`FRESH_MS`]) and, while an
//! app watches, when the pane has printed since its last read, which a change of the foreground
//! program always does; those reads are coalesced on a slow tick ([`INTERVAL_MS`]). An idle shell
//! costs nothing.
//!
//! Nothing here holds the Core lock across the supervisor (ADR 0079). A scan is a background job
//! that asks the supervisor and matches what it answers; its result is applied under the lock.
//! `agent.list` never fails or waits for the sake of a terminal: when a scan cannot run or fails
//! it answers with what is known.

mod harnesses;

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    offload::Offload,
    supervisor_handle::Supervisor,
    tick_due::Next,
};
use soyeht_protocol::{Pane, PaneMode, PaneState, TerminalHarness};
use soyeht_ptyd::wire::{MAX_FOREGROUND, Program, Request, Response};
use std::{cell::OnceCell, collections::BTreeSet, time::Duration};

/// The shortest time between two coalesced scans while an app watches.
const INTERVAL_MS: i64 = 2_000;
/// How recent a scan is good enough to answer `agent.list` with no new one.
const FRESH_MS: i64 = 1_000;
/// How long a request waits for a scan: the supervisor's answer.
const SCAN_TIMEOUT: Duration = Duration::from_secs(20);
/// The background job of the coalesced scan.
const SCAN_JOB: &str = "terminal.foreground";
/// The log event of a scan that failed on a request for `agent.list`.
const SCAN_FAILED: &str = "engine.foreground_scan_failed";

/// What the tick remembers of the scans.
#[derive(Default)]
pub(crate) struct Service {
    /// When the next coalesced scan may start, in the engine's clock.
    next_ms: i64,
    /// When the last scan was applied.
    scanned_ms: Option<i64>,
    /// The terminal panes that printed since they were last read.
    printed: BTreeSet<String>,
}

impl Service {
    /// Notes that pane `id` printed, so its foreground may have changed.
    pub(crate) fn note_output(&mut self, id: &str) {
        self.printed.insert(id.to_owned());
    }
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
}

/// What the supervisor and the table said of one pane.
struct Found {
    pane_id: String,
    instance_id: String,
    harness: Option<TerminalHarness>,
}

impl Scan {
    /// Asks the supervisor and matches the programs it reports. It runs with no lock. A pane the
    /// supervisor does not answer for keeps what it had: the supervisor could not read it, which
    /// does not say the harness left.
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
        // The person's `PATH` is asked for once, and only when a program needs it to be told.
        let path = OnceCell::new();
        let on_path = |folder: &str| {
            path.get_or_init(person_path_folders)
                .iter()
                .any(|listed| listed == folder)
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
                    harness: harness_of(&answer.programs, &on_path),
                })
            })
            .collect())
    }
}

/// The folders of the person's `PATH`, as the matcher compares them.
fn person_path_folders() -> Vec<String> {
    std::env::split_paths(&crate::agents::environment::launch_path())
        .map(|folder| harnesses::folders(&folder.to_string_lossy()))
        .collect()
}

/// The harness the programs of a terminal hold, with its version, if they hold one.
fn harness_of(programs: &[Program], on_path: &dyn Fn(&str) -> bool) -> Option<TerminalHarness> {
    let matched = harnesses::pick(programs, on_path)?;
    Some(TerminalHarness {
        id: matched.known.id.into(),
        name: matched.known.name.into(),
        version: matched.version(),
    })
}

impl Core {
    /// The terminal panes a scan asks the supervisor about (only those in `only`, when given), and
    /// the panes that keep a harness after their process ended, which are cleared with no
    /// question. A sign-in terminal runs a program Soyeht started for its agent, never a harness
    /// the person chose, so it is not asked about.
    fn foreground_targets(&mut self, only: Option<&BTreeSet<String>>) -> Result<Vec<Target>> {
        let mut targets = Vec::new();
        let mut gone = Vec::new();
        for (id, running) in &self.panes {
            let stored = &running.stored;
            if stored.closed
                || stored.pane.mode != PaneMode::Terminal
                || self.agents.has(id)
                || self.agents.is_login_terminal(id)
            {
                continue;
            }
            match &stored.instance_id {
                Some(instance_id) if stored.pane.state != PaneState::Dead => {
                    if only.is_none_or(|only| only.contains(id)) && targets.len() < MAX_FOREGROUND {
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
        if !supervisor.reports_foreground() {
            return Ok(None);
        }
        Ok(Some(Scan {
            supervisor,
            targets,
        }))
    }

    /// Whether an app subscribed to events watches the panes.
    fn watched(&self) -> bool {
        self.clients.values().any(|client| client.events)
    }

    /// The tick's scan: when an app watches and panes printed since they were read, reads those
    /// panes, at most once per [`INTERVAL_MS`].
    pub(crate) fn advance_foreground(&mut self) -> Result<()> {
        let now = self.now_ms();
        if !self.watched()
            || self.foreground().printed.is_empty()
            || now < self.foreground().next_ms
            || self.background.is_active(SCAN_JOB)
        {
            return Ok(());
        }
        let printed = std::mem::take(&mut self.foreground_mut().printed);
        self.foreground_mut().next_ms = now + INTERVAL_MS;
        let targets = self.foreground_targets(Some(&printed))?;
        if targets.is_empty() {
            return Ok(());
        }
        let Some(scan) = self.start_scan(targets)? else {
            return Ok(());
        };
        let used = scan.supervisor.clone();
        self.spawn_background(
            SCAN_JOB.into(),
            move || scan.run(),
            move |core, scanned| core.finish_foreground(&used, scanned),
        )
    }

    /// When the tick must run for the scan: while an app watches and a pane printed.
    pub(crate) fn foreground_due(&self, next: &mut Next, now: i64) {
        if self.watched() && !self.foreground().printed.is_empty() {
            next.at_ms(now, self.foreground().next_ms);
        }
    }

    /// Puts a scan's findings into the panes, for each that is still the process it read. A scan
    /// that failed on a link that was replaced meanwhile says nothing about the new one.
    fn finish_foreground(&mut self, used: &Supervisor, scanned: Result<Vec<Found>>) -> Result<()> {
        let Some(found) = self.settle_if_current(used, scanned)? else {
            return Ok(());
        };
        self.foreground_mut().scanned_ms = Some(self.now_ms());
        for Found {
            pane_id,
            instance_id,
            harness,
        } in found
        {
            self.foreground_mut().printed.remove(&pane_id);
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

    /// A scan of every terminal pane, or `None` when there is nothing to ask or the question
    /// cannot be put (no supervisor, an old one): the caller answers with what it knows. A failure
    /// is logged, once per change of kind.
    fn scan_of_every_terminal(&mut self) -> Option<Scan> {
        let made = self.foreground_targets(None).and_then(|targets| {
            if targets.is_empty() {
                Ok(None)
            } else {
                self.start_scan(targets)
            }
        });
        match made {
            Ok(scan) => scan,
            Err(error) => {
                self.settle_supervisor_call(SCAN_FAILED, Err(error));
                None
            }
        }
    }

    /// `agent.list`: the agent panes and the terminal panes that run a known harness (see
    /// `peer_listing`). The terminals are as fresh as [`FRESH_MS`], by a scan the reply waits for.
    /// A terminal never makes the list fail: with no supervisor, an old one or a scan that fails,
    /// it answers with what is known.
    pub(crate) fn agent_list(&mut self, sender: Pane) -> Result<Outcome> {
        let fresh = self
            .foreground()
            .scanned_ms
            .is_some_and(|at| self.now_ms().saturating_sub(at) < FRESH_MS);
        let scan = if fresh {
            None
        } else {
            self.scan_of_every_terminal()
        };
        let Some(scan) = scan else {
            return Ok(Outcome::Reply(self.peer_listing(&sender)));
        };
        let used = scan.supervisor.clone();
        Ok(Outcome::Offload(
            Offload::new(
                "Terminal scan timed out",
                SCAN_TIMEOUT,
                move || scan.run(),
                move |core, scanned| {
                    let applied = core.finish_foreground(&used, scanned);
                    core.settle_supervisor_call(SCAN_FAILED, applied);
                    Ok(Outcome::Reply(core.peer_listing(&sender)))
                },
            )
            .concurrent(),
        ))
    }
}

#[cfg(test)]
mod tests;

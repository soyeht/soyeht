//! Panes for unit tests: a bare one to build on, and a running terminal with no process behind it.

use crate::{
    agents::Running,
    core::Core,
    model::{RunningPane, StoredPane},
    pane_observations::SegmentStore,
    panes::persist_new_pane,
    ports::{StorePort, SupervisorPort, testing::FixedClock},
    screen::Screen,
    supervisor_link::tests::short_directory,
};
use serde_json::json;
use soyeht_pane_state::{EngineBuild, HarnessIdentity, ObservationPaneKind, RecordedRow};
use soyeht_protocol::{Pane, PaneMode, PaneState, StateSource, Workspace};
use soyeht_ptyd::{
    Error as PtydError,
    wire::{Instance, ProcessKind, Request, Response, Spawn},
    wire_kind,
};
use std::{collections::VecDeque, path::Path, sync::Arc};

/// Adds the pane `{label}-pane` to `core`, with its experiment capture `capture`, and returns
/// its id. Its screen is kept under `root`.
pub(crate) fn insert_terminal_pane(
    core: &mut Core,
    root: &Path,
    label: &str,
    capture: bool,
) -> String {
    let pane_id = format!("{label}-pane");
    let pane: Pane = serde_json::from_value(json!({
        "paneId": pane_id, "workspaceId": "workspace", "name": label,
        "cwd": root.display().to_string(), "mode": "terminal", "state": "unknown",
        "source": "none", "confidence": 0.0, "phrase": "Shell active", "generation": 1,
        "cols": 120, "rows": 40, "record": false, "pid": 1,
        "experimentCapture": {"on": capture}, "streamId": format!("{label}-stream"),
    }))
    .unwrap();
    let screen = Screen::open(&root.join("sessions").join(&pane_id), false).unwrap();
    core.panes.insert(
        pane_id.clone(),
        RunningPane {
            stored: StoredPane {
                pane,
                closed: false,
                instance_key: format!("{label}-key"),
                instance_id: Some(format!("{label}-instance")),
                supervisor_id: Some(format!("{label}-supervisor")),
                sequence_base: 0,
            },
            screen,
        },
    );
    pane_id
}

/// Every row the terminal capture of `pane_id` under `root` recorded, across its segments.
pub(crate) fn replay_rows(root: &Path, pane_id: &str, build: EngineBuild) -> Vec<RecordedRow> {
    SegmentStore::open(
        root,
        pane_id,
        ObservationPaneKind::Terminal,
        HarnessIdentity::Unknown,
        build,
    )
    .unwrap()
    .replay()
    .unwrap()
    .into_iter()
    .flat_map(|segment| segment.rows)
    .collect()
}

/// The state of an agent that is ready for work, with nothing queued or open.
pub(crate) fn ready_agent_state() -> crate::agents::state::State {
    use crate::agents::state::{Phase, State};
    let mut state = State::new(
        soyeht_protocol::HarnessLaunch {
            id: "test".into(),
            name: "Test".into(),
            command: "/test".into(),
            args: Vec::new(),
            version: "1".into(),
            environment: None,
        },
        "digest".into(),
        None,
    )
    .unwrap();
    state.phase = Phase::Ready;
    state.requested_options.clear();
    state
}

/// An idle pane with no optional field set: a test overrides what it exercises with
/// `Pane { name: .., ..pane(..) }`.
pub(crate) fn pane(id: &str, workspace_id: &str, mode: PaneMode, cwd: &str) -> Pane {
    Pane {
        pane_id: id.into(),
        workspace_id: workspace_id.into(),
        name: id.into(),
        cwd: cwd.into(),
        mode,
        state: PaneState::Idle,
        source: StateSource::Protocol,
        confidence: 1.0,
        phrase: String::new(),
        generation: 1,
        cols: 80,
        rows: 24,
        record: false,
        experiment_capture: None,
        pid: None,
        exit_code: None,
        stream_id: None,
        agent: None,
        url: None,
        title: None,
        viewport: None,
        incognito: None,
        web_session: None,
        writing_kind: None,
        writing_section: None,
    }
}

/// The request to start a terminal process with the instance key `key`.
pub(crate) fn pty_spawn(key: &str) -> Spawn {
    Spawn {
        instance_key: key.into(),
        generation: 1,
        kind: ProcessKind::Pty,
        cwd: "/".into(),
        argv: Vec::new(),
        env: Default::default(),
        cols: 80,
        rows: 24,
    }
}

/// A supervisor that fails the next spawns with the kinds it is given, starts the one
/// after them, reports `instance-0` as missing when drained, and answers the rest with `Ok`.
pub(crate) struct Spawning {
    pub(crate) failures: VecDeque<&'static str>,
    pub(crate) spawns: usize,
    /// The kind every close fails with, when set.
    pub(crate) close_failure: Option<&'static str>,
}

impl SupervisorPort for Spawning {
    fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response> {
        match request {
            Request::Spawn(config) => {
                self.spawns += 1;
                if let Some(kind) = self.failures.pop_front() {
                    return Err(PtydError::Remote {
                        kind: kind.into(),
                        message: "x".into(),
                    });
                }
                Ok(Response::Instance(Instance {
                    instance_id: format!("instance-{}", self.spawns),
                    supervisor_id: "spawning".into(),
                    spawn: config,
                    pid: 7,
                    closed: false,
                    exit_code: None,
                }))
            }
            // The agent's process is gone from the registry: the first drain says so.
            Request::Drain { instance_id, .. } if instance_id == "instance-0" => {
                Err(PtydError::Remote {
                    kind: wire_kind::NOT_FOUND.into(),
                    message: "x".into(),
                })
            }
            Request::Close { .. } if self.close_failure.is_some() => Err(PtydError::Remote {
                kind: self.close_failure.unwrap_or_default().into(),
                message: "x".into(),
            }),
            _ => Ok(Response::Ok),
        }
    }

    fn supervisor_id(&self) -> &str {
        "spawning"
    }

    fn supervisor_generation(&self) -> u32 {
        1
    }
}

/// A core with an agent pane that has a process, linked to a supervisor that fails the
/// next spawns with `failures`; the clock is the test's.
pub(crate) fn core_with_agent(failures: &[&'static str]) -> (Core, std::path::PathBuf) {
    let directory = short_directory();
    let state = crate::test_panes::ready_agent_state();
    let cwd = directory.to_string_lossy().into_owned();
    let pane = Pane {
        record: true,
        agent: Some(state.public(0)),
        ..crate::test_panes::pane("agent", "workspace", PaneMode::Agent, &cwd)
    };
    let saved = StoredPane {
        pane,
        closed: false,
        instance_key: "key".into(),
        instance_id: Some("instance-0".into()),
        supervisor_id: Some("spawning".into()),
        sequence_base: 0,
    };
    let workspace = Workspace {
        workspace_id: "workspace".into(),
        name: "W".into(),
        path: cwd,
        layout: json!({}),
    };
    let mut core = Core::open(&directory).unwrap();
    StorePort::mutate(&mut core.store, "test", "seed", "seed", |change| {
        change.insert_workspace(&workspace)?;
        persist_new_pane(change, &saved, Some(&state))?;
        Ok((json!({}), Vec::new()))
    })
    .unwrap();
    let screen = Screen::open(&directory.join("sessions/agent"), true).unwrap();
    core.panes.insert(
        "agent".into(),
        RunningPane {
            stored: saved,
            screen,
        },
    );
    core.agents.insert("agent".into(), Running::new(state));
    core.clock = Arc::new(FixedClock(1_000_000));
    core.ptyd = Some(Box::new(Spawning {
        failures: failures.iter().copied().collect(),
        spawns: 0,
        close_failure: None,
    }));
    (core, directory)
}

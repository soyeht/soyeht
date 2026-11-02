//! The hooks of the engine integration tests and the end-to-end suite (ADR 0091).
//!
//! Compiled only with the `test-hooks` feature, and installed only by `soyeht-engine-test`.
//! A test steers the running engine by dropping a marker file in its data directory, or by
//! setting an environment variable before it starts. No shipped engine contains this module,
//! so no file or variable changes how it behaves.

use crate::{
    core::Core,
    editor::{
        FileSystemPort,
        interfered_files::{InterferedFiles, Interference},
    },
    errors,
    hooks::Hooks,
    model::RunningPane,
    usage::FixtureHost,
};
use soyeht_ptyd::wire::Request;
use soyeht_store::segments::Record;
use soyeht_usage::host::Host;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Fails the poll of the agent pane named in the file.
const FAIL_AGENT_POLL: &str = "test-fail-agent-poll";
/// Closes the polled agent's process before its poll, once; the file is removed.
const DROP_AGENT_BEFORE_POLL: &str = "test-drop-current-agent-before-poll";
/// Appends one gap to the candidate's screen just before a harness switch commits.
const ADVANCE_SCREEN_BEFORE_SWITCH: &str = "test-advance-screen-before-harness-switch-commit";
/// Makes closing the process a harness switch retired fail.
const FAIL_OLD_CLOSE: &str = "test-fail-harness-switch-old-close";
/// Holds the editor's autosaves.
const PAUSE_EDITOR_AUTOSAVE: &str = "test-pause-editor-autosave";
/// Names the directory of the usage fixtures (see [`FixtureHost`]).
const USAGE_FIXTURES_VARIABLE: &str = "SOYEHT_TEST_USAGE_READER_DIR";
/// Reads skill folders on the probe instead of by file events, which a loaded machine delivers
/// late.
const POLL_SKILLS_VARIABLE: &str = "SOYEHT_TEST_SKILL_POLL";
/// Milliseconds every flush of a terminal segment to the disk takes, as on a busy machine.
const SLOW_DISK_VARIABLE: &str = "SOYEHT_TEST_SLOW_DISK_MS";

/// Milliseconds starting a file-event stream takes, as on a busy machine.
const SLOW_WATCH_VARIABLE: &str = "SOYEHT_TEST_SLOW_WATCH_MS";

pub struct FileHooks {
    directory: PathBuf,
    slow_disk: bool,
    usage_fixtures: Option<PathBuf>,
    poll_skills: bool,
}

impl FileHooks {
    /// The hooks of an engine whose data lives in `directory`, with the environment it started in.
    pub fn from_environment(directory: &Path) -> Self {
        let mut slow_disk = false;
        if let Ok(value) = std::env::var(SLOW_DISK_VARIABLE)
            && let Ok(pause) = value.parse()
        {
            soyeht_store::segments::slow_disk::set(std::time::Duration::from_millis(pause));
            slow_disk = true;
        }
        if let Ok(value) = std::env::var(SLOW_WATCH_VARIABLE)
            && let Ok(pause) = value.parse()
        {
            crate::file_events::slow_start::set(pause);
        }
        Self {
            directory: directory.into(),
            slow_disk,
            usage_fixtures: std::env::var_os(USAGE_FIXTURES_VARIABLE).map(PathBuf::from),
            poll_skills: std::env::var_os(POLL_SKILLS_VARIABLE).is_some(),
        }
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
}

impl Hooks for FileHooks {
    fn before_agent_poll(&self, core: &mut Core, pane: &str) -> errors::Result<()> {
        if let Ok(target) = std::fs::read_to_string(self.marker(FAIL_AGENT_POLL))
            && target.trim() == pane
        {
            return Err(errors::not_found("Injected per-pane polling failure"));
        }
        let marker = self.marker(DROP_AGENT_BEFORE_POLL);
        if marker.exists() {
            std::fs::remove_file(marker).map_err(errors::io)?;
            let instance_id = core.panes[pane]
                .stored
                .instance_id
                .clone()
                .ok_or_else(|| errors::internal("Agent process has no adopted instance"))?;
            // A hook of a test engine: it closes the process under the lock on purpose.
            let link = core.supervisor()?;
            let closed = link.call(Request::Close { instance_id });
            core.settle_supervisor(&link, closed)?;
        }
        Ok(())
    }

    fn before_switch_commit(&self, pane: &mut RunningPane) -> errors::Result<()> {
        if self.marker(ADVANCE_SCREEN_BEFORE_SWITCH).exists() {
            let from = pane.screen.next_offset();
            pane.screen.append(Record::Gap {
                from,
                to: from.saturating_add(1),
            })?;
        }
        Ok(())
    }

    fn fail_old_close(&self) -> bool {
        self.marker(FAIL_OLD_CLOSE).is_file()
    }

    fn pause_editor_autosave(&self) -> bool {
        self.marker(PAUSE_EDITOR_AUTOSAVE).is_file()
    }

    fn poll_skills_only(&self) -> bool {
        self.poll_skills
    }

    fn editor_disk(&self) -> Option<Box<dyn FileSystemPort>> {
        self.slow_disk
            .then(|| Box::new(InterferedFiles(SlowWrites)) as Box<dyn FileSystemPort>)
    }

    fn usage_host(&self) -> Option<Arc<dyn Host>> {
        let directory = self.usage_fixtures.clone()?;
        Some(Arc::new(FixtureHost::new(directory)))
    }
}

/// Makes every write of the editor take as long as the slow-disk setting says (the pause that
/// delays the flush of a terminal segment), so a test can show what waits for a save.
struct SlowWrites;

impl Interference for SlowWrites {
    fn before_write(&self, _path: &Path) -> io::Result<()> {
        soyeht_store::segments::slow_disk::wait();
        Ok(())
    }
}

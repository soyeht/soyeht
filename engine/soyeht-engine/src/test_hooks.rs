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
    pairing_approval::Approval,
    pairing_peers::PairingPeers,
    ports::{PortScanError, PortScanObservation, PortScanPort, SupervisorPort},
    usage::FixtureHost,
};
mod peers;
pub use peers::{
    FIXTURE_COMPUTER_NODE, FIXTURE_NODE_KEY, FIXTURE_PHONE_ADDRESS, FIXTURE_PHONE_NODE,
    FIXTURE_TAILNET, FixturePeers, fixture_identity,
};
use soyeht_ink::Recognizer;
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
/// While this marker exists every local client counts as the signed Mac app.
const ATTEST_LOCAL_UI: &str = "test-attest-local-ui";
/// Holds the way a pairing is approved (`signed-app`, `persons-code`, `refused` or `system`) in place
/// of the signed app's rule, which a test engine follows with no marker.
const PAIRING_APPROVAL: &str = "test-pairing-approval";
/// While this marker exists the pairing rules see every connection from the network as the
/// fixture phone's (see [`FixturePeers::phone`]); its text may say `whois-down` (Tailscale cannot
/// be asked) or `same-node` (Tailscale says the phone is this computer).
const PAIRING_PEERS: &str = "test-pairing-peers";
/// Names the directory of the usage fixtures (see [`FixtureHost`]).
const USAGE_FIXTURES_VARIABLE: &str = "SOYEHT_TEST_USAGE_READER_DIR";
/// Reads skill folders on the probe instead of by file events, which a loaded machine delivers
/// late.
const POLL_SKILLS_VARIABLE: &str = "SOYEHT_TEST_SKILL_POLL";
/// Milliseconds every flush of a terminal segment to the disk takes, as on a busy machine.
const SLOW_DISK_VARIABLE: &str = "SOYEHT_TEST_SLOW_DISK_MS";

/// Replaces the system's handwriting recognizer with one that reads every writing as one word
/// after pausing this many milliseconds, as a slow recognizer would.
const INK_RECOGNIZER_VARIABLE: &str = "SOYEHT_TEST_INK_RECOGNIZER_MS";

/// Milliseconds starting a file-event stream takes, as on a busy machine.
const SLOW_WATCH_VARIABLE: &str = "SOYEHT_TEST_SLOW_WATCH_MS";

/// Writes one log line for every hold of the Core lock and every request to the supervisor, with
/// the commits the stores have made so far (see [`crate::measuring`]).
const MEASURE_COST_VARIABLE: &str = "SOYEHT_TEST_MEASURE_COST";

/// Milliseconds the tick may sleep with no deadline and no event, in place of its second.
const TICK_CEILING_VARIABLE: &str = "SOYEHT_TEST_TICK_CEILING_MS";

/// Takes the scan of the local servers out of the engine: it cannot list, so it is never run.
const NO_PORT_SCAN_VARIABLE: &str = "SOYEHT_TEST_NO_PORT_SCAN";

/// Leaves the user's skill folder unfollowed (see [`Hooks::follows_user_skills`]).
const NO_USER_SKILLS_VARIABLE: &str = "SOYEHT_TEST_NO_USER_SKILLS";

struct NoPortScan;

impl PortScanPort for NoPortScan {
    fn unavailable(&self) -> Option<&'static str> {
        Some("the scan is out of this test engine")
    }

    fn scan(&mut self) -> Result<PortScanObservation, PortScanError> {
        Err(PortScanError::Unsupported(
            "the scan is out of this test engine",
        ))
    }
}

pub struct FileHooks {
    directory: PathBuf,
    slow_disk: bool,
    usage_fixtures: Option<PathBuf>,
    poll_skills: bool,
    ink_pause: Option<std::time::Duration>,
    tick_ceiling: Option<std::time::Duration>,
    no_port_scan: bool,
    no_user_skills: bool,
    measures_cost: bool,
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
        let mut ink_pause = None;
        if let Ok(value) = std::env::var(INK_RECOGNIZER_VARIABLE)
            && let Ok(pause) = value.parse()
        {
            ink_pause = Some(std::time::Duration::from_millis(pause));
        }
        let mut tick_ceiling = None;
        if let Ok(value) = std::env::var(TICK_CEILING_VARIABLE)
            && let Ok(milliseconds) = value.parse()
        {
            tick_ceiling = Some(std::time::Duration::from_millis(milliseconds));
        }
        Self {
            directory: directory.into(),
            slow_disk,
            tick_ceiling,
            no_port_scan: std::env::var_os(NO_PORT_SCAN_VARIABLE).is_some(),
            no_user_skills: std::env::var_os(NO_USER_SKILLS_VARIABLE).is_some(),
            measures_cost: std::env::var_os(MEASURE_COST_VARIABLE).is_some(),
            usage_fixtures: std::env::var_os(USAGE_FIXTURES_VARIABLE).map(PathBuf::from),
            poll_skills: std::env::var_os(POLL_SKILLS_VARIABLE).is_some(),
            ink_pause,
        }
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
}

impl Hooks for FileHooks {
    fn attests_local_ui(&self) -> bool {
        self.marker(ATTEST_LOCAL_UI).exists()
    }

    fn pairing_peers(&self) -> Option<Arc<dyn PairingPeers>> {
        let Ok(text) = std::fs::read_to_string(self.marker(PAIRING_PEERS)) else {
            return None;
        };
        let peers = FixturePeers::phone();
        match text.trim() {
            "whois-down" => peers.set_node(FIXTURE_PHONE_ADDRESS, None),
            "same-node" => peers.set_node(FIXTURE_PHONE_ADDRESS, Some(FIXTURE_COMPUTER_NODE)),
            _ => {}
        }
        Some(Arc::new(peers))
    }

    fn pairing_approval(&self) -> Option<Approval> {
        // The tests of pairing were written for the signed app's rule, which every system can
        // run, while Linux asks for the person's code by itself (ADR 0164): with no marker this
        // engine follows the signed app's, and `system` is the way to ask for the system's own.
        let Ok(text) = std::fs::read_to_string(self.marker(PAIRING_APPROVAL)) else {
            return Some(Approval::SignedApp);
        };
        match text.trim() {
            "signed-app" => Some(Approval::SignedApp),
            "persons-code" => Some(Approval::PersonsCode),
            "refused" => Some(Approval::Refused),
            _ => None,
        }
    }

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

    fn tick_ceiling(&self) -> Option<std::time::Duration> {
        self.tick_ceiling
    }

    fn follows_user_skills(&self) -> bool {
        !self.no_user_skills
    }

    fn port_scan(&self) -> Option<Box<dyn PortScanPort>> {
        self.no_port_scan
            .then(|| Box::new(NoPortScan) as Box<dyn PortScanPort>)
    }

    fn poll_skills_only(&self) -> bool {
        self.poll_skills
    }

    fn editor_disk(&self) -> Option<Box<dyn FileSystemPort>> {
        self.slow_disk
            .then(|| Box::new(InterferedFiles(SlowWrites)) as Box<dyn FileSystemPort>)
    }

    fn ink_recognizer(&self) -> Option<Box<dyn Recognizer>> {
        Some(Box::new(PausedRecognizer(self.ink_pause?)))
    }

    fn usage_host(&self) -> Option<Arc<dyn Host>> {
        let directory = self.usage_fixtures.clone()?;
        Some(Arc::new(FixtureHost::new(directory)))
    }

    fn watches_holds(&self) -> bool {
        self.measures_cost
    }

    fn lock_held(
        &self,
        purpose: &str,
        method: &str,
        held: std::time::Duration,
        waited: std::time::Duration,
    ) {
        crate::measuring::lock_held(purpose, method, held, waited);
    }

    fn supervisor_port(&self, port: Box<dyn SupervisorPort>) -> Box<dyn SupervisorPort> {
        if self.measures_cost {
            Box::new(crate::measuring::Counting(port))
        } else {
            port
        }
    }
}

/// A recognizer that takes `.0` to read and says whether it was started under the Core lock,
/// which is the one thing a test of `ink.recognize` needs to see (ADR 0079).
struct PausedRecognizer(std::time::Duration);

impl Recognizer for PausedRecognizer {
    fn id(&self) -> &'static str {
        "paused-fixture"
    }

    fn languages(&mut self) -> soyeht_ink::Result<Vec<String>> {
        Ok(vec!["en-US".into()])
    }

    fn read(
        &mut self,
        _image: &soyeht_ink::Bitmap,
        _languages: &[String],
        _most: usize,
    ) -> soyeht_ink::Result<Vec<soyeht_ink::Candidate>> {
        std::thread::sleep(self.0);
        let text = match soyeht_process::forbidden() {
            None => "off-the-lock",
            Some(_) => "under-the-lock",
        };
        Ok(vec![soyeht_ink::Candidate {
            text: text.into(),
            score: Some(1.0),
        }])
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

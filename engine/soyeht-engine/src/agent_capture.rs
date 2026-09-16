use crate::{
    core::Core,
    errors::{self, Result},
    pane_observations::{ObservationRecorder, SegmentStore},
};
use soyeht_pane_state::{EngineBuild, GroundTruthLabel, HarnessIdentity, ObservationPaneKind};
use soyeht_protocol::{PaneMode, StateSource};
use std::path::Path;

pub(crate) struct AgentCapture {
    recorder: ObservationRecorder,
    pane_id: String,
    harness: HarnessIdentity,
    next_sequence: u64,
}

impl AgentCapture {
    fn open(
        root: &Path,
        pane_id: &str,
        harness: HarnessIdentity,
        build: EngineBuild,
    ) -> Result<Self> {
        let store = SegmentStore::open(
            root,
            pane_id,
            ObservationPaneKind::Agent,
            harness.clone(),
            build,
        )?;
        let next_sequence = store.next_sequence()?;
        Ok(Self {
            recorder: ObservationRecorder::open(store)?,
            pane_id: pane_id.into(),
            harness,
            next_sequence,
        })
    }

    pub(crate) fn harness(&self) -> &HarnessIdentity {
        &self.harness
    }

    fn record(&mut self, timestamp_ms: i64, state: soyeht_protocol::PaneState) -> Result<()> {
        let label = GroundTruthLabel {
            pane_id: self.pane_id.clone(),
            pane_kind: ObservationPaneKind::Agent,
            harness: self.harness.clone(),
            timestamp_ms,
            sequence: self.next_sequence,
            state,
            source: StateSource::Protocol,
        };
        self.recorder.record_label(label)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| errors::internal("Agent label sequence exhausted"))?;
        Ok(())
    }

    pub(crate) fn failure_reason(&self) -> Result<Option<String>> {
        self.recorder.failure()
    }
}

impl Core {
    pub(crate) fn prepare_agent_capture(&mut self, pane_id: &str) -> Result<AgentCapture> {
        self.panes
            .get(pane_id)
            .filter(|pane| !pane.stored.closed && pane.stored.pane.mode == PaneMode::Agent)
            .ok_or_else(|| errors::not_found("Agent pane not found"))?;
        let state = self
            .agents
            .get(pane_id)
            .ok_or_else(|| errors::internal("Agent capture has no agent runtime"))?;
        let harness = HarnessIdentity::known(
            state.state.launch.id.clone(),
            Some(state.state.initialized.as_ref().map_or_else(
                || state.state.launch.version.clone(),
                |initialized| initialized.version.clone(),
            )),
        )?;
        let build = self.capture_build()?;
        AgentCapture::open(&self.observation_root()?, pane_id, harness, build)
    }

    pub(crate) fn record_agent_label(&mut self, pane_id: &str) -> Result<()> {
        let pane = self
            .panes
            .get(pane_id)
            .ok_or_else(|| errors::internal("Captured agent pane disappeared"))?;
        if pane.stored.pane.source != StateSource::Protocol {
            return Ok(());
        }
        let state = pane.stored.pane.state.clone();
        let expected_harness = self
            .agents
            .get(pane_id)
            .map(|running| {
                HarnessIdentity::known(
                    running.state.launch.id.clone(),
                    Some(running.state.initialized.as_ref().map_or_else(
                        || running.state.launch.version.clone(),
                        |initialized| initialized.version.clone(),
                    )),
                )
            })
            .transpose()?
            .ok_or_else(|| errors::internal("Captured agent pane has no runtime"))?;
        if self
            .agent_captures
            .get(pane_id)
            .is_some_and(|capture| capture.harness() != &expected_harness)
        {
            // Close the previous recorder before the new harness opens its
            // segment so the old provenance receives its own clean footer.
            self.agent_captures.remove(pane_id);
            match self.prepare_agent_capture(pane_id) {
                Ok(capture) => {
                    self.agent_captures.insert(pane_id.into(), capture);
                }
                Err(error) => {
                    self.fail_experiment_capture(pane_id, Some(error.to_string()))?;
                    return Ok(());
                }
            }
        }
        let timestamp_ms = self.now_ms();
        let failure = self
            .agent_captures
            .get_mut(pane_id)
            .and_then(|capture| capture.record(timestamp_ms, state).err())
            .map(|error| error.to_string());
        if let Some(failure) = failure {
            self.fail_experiment_capture(pane_id, Some(failure))?;
        }
        Ok(())
    }
}

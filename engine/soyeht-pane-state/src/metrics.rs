use crate::{Detector, HarnessIdentity, ObservationGap, RecordedRow, Result};
use serde::Serialize;
use soyeht_protocol::PaneState;

const COVERAGE_COMPARISON_BASIS_POINTS: u16 = 500;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    pub numerator: u64,
    pub denominator: u64,
    pub value: Option<f64>,
}

impl Rate {
    fn new(numerator: u64, denominator: u64) -> Self {
        Self {
            numerator,
            denominator,
            value: (denominator > 0).then(|| numerator as f64 / denominator as f64),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyMetrics {
    pub measured: u64,
    pub unavailable: u64,
    pub median_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    pub maximum_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateMetrics {
    pub state: PaneState,
    pub true_positive: u64,
    pub false_positive: u64,
    pub false_negative: u64,
    pub precision: Rate,
    pub recall: Rate,
    pub time_coverage: Option<Rate>,
    pub latency: LatencyMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessMetrics {
    pub harness: HarnessIdentity,
    pub panes: u64,
    pub observations: u64,
    pub labeled_observations: u64,
    pub unknown_rate: Rate,
    pub time_coverage: Option<Rate>,
    pub unclassified_gap_ms: u64,
    pub clock_discontinuities: u64,
    pub states: Vec<StateMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsReport {
    pub harnesses: Vec<HarnessMetrics>,
}

impl MetricsReport {
    /// Candidate results are comparable only when they cover the same harnesses
    /// and their measured wall-clock coverage differs by no more than five
    /// percentage points. Missing coverage is never treated as zero loss.
    pub fn can_rank_against(&self, other: &Self) -> bool {
        if self.harnesses.len() != other.harnesses.len() {
            return false;
        }
        self.harnesses.iter().all(|left| {
            let Some(right) = other
                .harnesses
                .iter()
                .find(|right| right.harness == left.harness)
            else {
                return false;
            };
            let (Some(left), Some(right)) = (&left.time_coverage, &right.time_coverage) else {
                return false;
            };
            if left.denominator == 0 || right.denominator == 0 {
                return false;
            }
            let left_scaled = u128::from(left.numerator) * u128::from(right.denominator);
            let right_scaled = u128::from(right.numerator) * u128::from(left.denominator);
            let difference = left_scaled.abs_diff(right_scaled);
            let denominator = u128::from(left.denominator) * u128::from(right.denominator);
            difference * 10_000 <= u128::from(COVERAGE_COMPARISON_BASIS_POINTS) * denominator
        })
    }
}

#[derive(Clone)]
struct TraceRow {
    row: RecordedRow,
    sequence: u64,
}

struct PaneTrace {
    pane_id: String,
    harness: HarnessIdentity,
    rows: Vec<TraceRow>,
}

#[derive(Default)]
struct MutableState {
    true_positive: u64,
    false_positive: u64,
    false_negative: u64,
    total_ms: u64,
    lost_ms: u64,
    latencies: Vec<u64>,
    latency_unavailable: u64,
}

struct MutableHarness {
    harness: HarnessIdentity,
    panes: u64,
    observations: u64,
    labeled_observations: u64,
    unknown: u64,
    total_ms: u64,
    lost_ms: u64,
    coverage_available: bool,
    unclassified_gap_ms: u64,
    clock_discontinuities: u64,
    states: Vec<(PaneState, MutableState)>,
}

impl MutableHarness {
    fn new(harness: HarnessIdentity) -> Self {
        Self {
            harness,
            panes: 0,
            observations: 0,
            labeled_observations: 0,
            unknown: 0,
            total_ms: 0,
            lost_ms: 0,
            coverage_available: true,
            unclassified_gap_ms: 0,
            clock_discontinuities: 0,
            states: Vec::new(),
        }
    }

    fn state_mut(&mut self, state: &PaneState) -> &mut MutableState {
        if let Some(index) = self.states.iter().position(|(item, _)| item == state) {
            return &mut self.states[index].1;
        }
        self.states.push((state.clone(), MutableState::default()));
        let index = self.states.len() - 1;
        &mut self.states[index].1
    }

    fn finish(mut self) -> HarnessMetrics {
        self.states.sort_by_key(|(state, _)| state_order(state));
        let states = self
            .states
            .into_iter()
            .map(|(state, mut metrics)| {
                metrics.latencies.sort_unstable();
                let latency = LatencyMetrics {
                    measured: metrics.latencies.len() as u64,
                    unavailable: metrics.latency_unavailable,
                    median_ms: percentile(&metrics.latencies, 50),
                    p95_ms: percentile(&metrics.latencies, 95),
                    maximum_ms: metrics.latencies.last().copied(),
                };
                StateMetrics {
                    state,
                    true_positive: metrics.true_positive,
                    false_positive: metrics.false_positive,
                    false_negative: metrics.false_negative,
                    precision: Rate::new(
                        metrics.true_positive,
                        metrics.true_positive.saturating_add(metrics.false_positive),
                    ),
                    recall: Rate::new(
                        metrics.true_positive,
                        metrics.true_positive.saturating_add(metrics.false_negative),
                    ),
                    time_coverage: self.coverage_available.then(|| {
                        Rate::new(
                            metrics.total_ms.saturating_sub(metrics.lost_ms),
                            metrics.total_ms,
                        )
                    }),
                    latency,
                }
            })
            .collect();
        HarnessMetrics {
            harness: self.harness,
            panes: self.panes,
            observations: self.observations,
            labeled_observations: self.labeled_observations,
            unknown_rate: Rate::new(self.unknown, self.observations),
            time_coverage: self
                .coverage_available
                .then(|| Rate::new(self.total_ms.saturating_sub(self.lost_ms), self.total_ms)),
            unclassified_gap_ms: self.unclassified_gap_ms,
            clock_discontinuities: self.clock_discontinuities,
            states,
        }
    }
}

/// Scores one detector against labels in the same ordered observation stream.
/// Labels remain separate from frames, so the detector cannot inspect them.
pub fn evaluate(detector: &dyn Detector, rows: &[RecordedRow]) -> Result<MetricsReport> {
    let mut traces: Vec<PaneTrace> = Vec::new();
    for row in rows {
        row.validate()?;
        let (pane_id, harness, sequence) = identity(row);
        let index = traces
            .iter()
            .position(|trace| trace.pane_id == pane_id && trace.harness == *harness)
            .unwrap_or_else(|| {
                traces.push(PaneTrace {
                    pane_id: pane_id.into(),
                    harness: harness.clone(),
                    rows: Vec::new(),
                });
                traces.len() - 1
            });
        traces[index].rows.push(TraceRow {
            row: row.clone(),
            sequence,
        });
    }
    for trace in &mut traces {
        trace.rows.sort_by_key(|row| row.sequence);
        if trace
            .rows
            .windows(2)
            .any(|pair| pair[0].sequence >= pair[1].sequence)
        {
            return Err(crate::Error::InvalidSequence);
        }
    }

    let mut harnesses: Vec<MutableHarness> = Vec::new();
    for trace in traces {
        let index = harnesses
            .iter()
            .position(|metrics| metrics.harness == trace.harness)
            .unwrap_or_else(|| {
                harnesses.push(MutableHarness::new(trace.harness.clone()));
                harnesses.len() - 1
            });
        score_trace(detector, &trace, &mut harnesses[index]);
    }
    harnesses.sort_by(|left, right| harness_name(&left.harness).cmp(harness_name(&right.harness)));
    Ok(MetricsReport {
        harnesses: harnesses.into_iter().map(MutableHarness::finish).collect(),
    })
}

fn score_trace(detector: &dyn Detector, trace: &PaneTrace, metrics: &mut MutableHarness) {
    metrics.panes += 1;
    let observations: Vec<_> = trace
        .rows
        .iter()
        .filter_map(|row| match &row.row {
            RecordedRow::Observation(frame) => Some((row.sequence, frame, detector.state(frame))),
            _ => None,
        })
        .collect();
    let labels: Vec<_> = trace
        .rows
        .iter()
        .filter_map(|row| match &row.row {
            RecordedRow::Label(label) => Some((row.sequence, label)),
            _ => None,
        })
        .collect();
    metrics.observations += observations.len() as u64;
    metrics.unknown += observations
        .iter()
        .filter(|(_, _, detected)| detected.state == PaneState::Unknown)
        .count() as u64;

    for (sequence, _, detected) in &observations {
        let truth = labels
            .iter()
            .rev()
            .find(|(label_sequence, _)| label_sequence <= sequence)
            .map(|(_, label)| &label.state);
        let Some(truth) = truth else { continue };
        metrics.labeled_observations += 1;
        if &detected.state == truth {
            metrics.state_mut(truth).true_positive += 1;
        } else {
            metrics.state_mut(truth).false_negative += 1;
            if detected.state != PaneState::Unknown {
                metrics.state_mut(&detected.state).false_positive += 1;
            }
        }
    }

    score_latency(&observations, &labels, metrics);
    score_coverage(trace, &labels, metrics);
}

fn score_latency(
    observations: &[(u64, &crate::ObservationFrame, crate::Detection)],
    labels: &[(u64, &crate::GroundTruthLabel)],
    metrics: &mut MutableHarness,
) {
    for (index, (label_sequence, label)) in labels.iter().enumerate() {
        let next_label = labels.get(index + 1).map(|(sequence, _)| *sequence);
        let matching = observations.iter().find(|(sequence, _, detected)| {
            sequence > label_sequence
                && next_label.is_none_or(|next| *sequence < next)
                && detected.state == label.state
        });
        let state = metrics.state_mut(&label.state);
        let Some((_, frame, _)) = matching else {
            state.latency_unavailable += 1;
            continue;
        };
        if frame.timestamp_ms < label.timestamp_ms {
            state.latency_unavailable += 1;
        } else {
            state
                .latencies
                .push((frame.timestamp_ms - label.timestamp_ms) as u64);
        }
    }
}

fn score_coverage(
    trace: &PaneTrace,
    labels: &[(u64, &crate::GroundTruthLabel)],
    metrics: &mut MutableHarness,
) {
    let Some(first) = trace.rows.first() else {
        return;
    };
    let Some(last) = trace.rows.last() else {
        return;
    };
    let first_timestamp = row_start_timestamp(&first.row);
    let last_timestamp = row_end_timestamp(&last.row);
    if last_timestamp < first_timestamp {
        metrics.coverage_available = false;
        metrics.clock_discontinuities += 1;
        return;
    }
    let total = (last_timestamp - first_timestamp) as u64;
    metrics.total_ms = metrics.total_ms.saturating_add(total);

    for pair in trace.rows.windows(2) {
        if row_start_timestamp(&pair[1].row) < row_end_timestamp(&pair[0].row) {
            metrics.coverage_available = false;
            metrics.clock_discontinuities += 1;
        }
    }

    for (index, (_, label)) in labels.iter().enumerate() {
        let end = labels
            .get(index + 1)
            .map_or(last_timestamp, |(_, next)| next.timestamp_ms);
        if end < label.timestamp_ms {
            metrics.coverage_available = false;
            metrics.clock_discontinuities += 1;
            continue;
        }
        let state = metrics.state_mut(&label.state);
        state.total_ms = state
            .total_ms
            .saturating_add((end - label.timestamp_ms) as u64);
    }

    for row in &trace.rows {
        let RecordedRow::Gap(gap) = &row.row else {
            continue;
        };
        let Some(duration) = gap_duration(gap) else {
            metrics.coverage_available = false;
            metrics.clock_discontinuities += 1;
            continue;
        };
        metrics.lost_ms = metrics.lost_ms.saturating_add(duration);
        let before = labels
            .iter()
            .rev()
            .find(|(sequence, _)| *sequence < gap.first_sequence)
            .map(|(_, label)| &label.state);
        let after = labels
            .iter()
            .find(|(sequence, _)| *sequence > gap.last_sequence)
            .map(|(_, label)| &label.state);
        if let (Some(before), Some(after)) = (before, after)
            && before == after
        {
            let state = metrics.state_mut(before);
            state.lost_ms = state.lost_ms.saturating_add(duration);
        } else {
            metrics.unclassified_gap_ms = metrics.unclassified_gap_ms.saturating_add(duration);
        }
    }
}

fn gap_duration(gap: &ObservationGap) -> Option<u64> {
    (gap.last_timestamp_ms >= gap.first_timestamp_ms)
        .then(|| (gap.last_timestamp_ms - gap.first_timestamp_ms) as u64)
}

fn identity(row: &RecordedRow) -> (&str, &HarnessIdentity, u64) {
    match row {
        RecordedRow::Observation(frame) => (&frame.pane_id, &frame.harness, frame.sequence),
        RecordedRow::Label(label) => (&label.pane_id, &label.harness, label.sequence),
        RecordedRow::Gap(gap) => (&gap.pane_id, &gap.harness, gap.last_sequence),
    }
}

fn row_start_timestamp(row: &RecordedRow) -> i64 {
    match row {
        RecordedRow::Observation(frame) => frame.timestamp_ms,
        RecordedRow::Label(label) => label.timestamp_ms,
        RecordedRow::Gap(gap) => gap.first_timestamp_ms,
    }
}

fn row_end_timestamp(row: &RecordedRow) -> i64 {
    match row {
        RecordedRow::Observation(frame) => frame.timestamp_ms,
        RecordedRow::Label(label) => label.timestamp_ms,
        RecordedRow::Gap(gap) => gap.last_timestamp_ms,
    }
}

fn percentile(values: &[u64], percent: usize) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let index = (values.len() * percent).div_ceil(100).saturating_sub(1);
    values.get(index).copied()
}

fn harness_name(harness: &HarnessIdentity) -> &str {
    harness.id().unwrap_or("")
}

fn state_order(state: &PaneState) -> u8 {
    match state {
        PaneState::Starting => 0,
        PaneState::Working => 1,
        PaneState::Idle => 2,
        PaneState::WaitingForYou => 3,
        PaneState::Cancelling => 4,
        PaneState::NoSignal => 5,
        PaneState::Recovering => 6,
        PaneState::Dead => 7,
        PaneState::Unknown => 8,
    }
}

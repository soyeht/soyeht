use crate::{Detector, HarnessIdentity, ObservationGap, ObservationPaneKind, RecordedRow, Result};
use serde::Serialize;
use soyeht_protocol::{PaneState, StateSource};
use std::collections::BTreeSet;

const COVERAGE_COMPARISON_BASIS_POINTS: u16 = 500;
/// Descriptive evidence floor for a truth-source group and for each state.
pub const MIN_GROUND_TRUTH_TRANSITIONS: u64 = 20;
/// A published precision or recall value needs this many comparisons.
pub const MIN_RATE_DENOMINATOR: u64 = 20;

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

    fn score(numerator: u64, denominator: u64, group_ready: bool) -> Self {
        Self {
            numerator,
            denominator,
            value: (group_ready && denominator >= MIN_RATE_DENOMINATOR)
                .then(|| numerator as f64 / denominator as f64),
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
pub struct StateDurationMetrics {
    pub measured: u64,
    pub unavailable: u64,
    pub minimum_ms: Option<u64>,
    pub median_ms: Option<u64>,
    pub entries_under_one_second: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateMetrics {
    pub state: PaneState,
    pub truth_transitions: u64,
    pub true_positive: u64,
    pub false_positive: u64,
    pub false_negative: u64,
    pub precision: Rate,
    pub recall: Rate,
    pub time_coverage: Option<Rate>,
    pub latency: LatencyMetrics,
    pub duration: StateDurationMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessMetrics {
    pub pane_kind: ObservationPaneKind,
    pub harness: HarnessIdentity,
    pub ground_truth_source: Option<StateSource>,
    pub panes: u64,
    pub observations: u64,
    pub labels: u64,
    pub label_transitions: u64,
    pub labeled_observations: u64,
    pub scores_available: bool,
    pub panes_with_omitted_sources: u64,
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
            let Some(right) = other.harnesses.iter().find(|right| {
                right.pane_kind == left.pane_kind
                    && right.harness == left.harness
                    && right.ground_truth_source == left.ground_truth_source
            }) else {
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
    pane_kind: ObservationPaneKind,
    harness: HarnessIdentity,
    rows: Vec<TraceRow>,
}

#[derive(Default)]
struct MutableState {
    truth_transitions: u64,
    true_positive: u64,
    false_positive: u64,
    false_negative: u64,
    total_ms: u64,
    lost_ms: u64,
    latencies: Vec<u64>,
    latency_unavailable: u64,
    durations: Vec<u64>,
    duration_unavailable: u64,
}

struct MutableHarness {
    pane_kind: ObservationPaneKind,
    harness: HarnessIdentity,
    ground_truth_source: Option<StateSource>,
    panes: u64,
    observations: u64,
    labels: u64,
    label_transitions: u64,
    labeled_observations: u64,
    panes_with_omitted_sources: u64,
    unknown: u64,
    total_ms: u64,
    lost_ms: u64,
    coverage_available: bool,
    unclassified_gap_ms: u64,
    clock_discontinuities: u64,
    states: Vec<(PaneState, MutableState)>,
}

impl MutableHarness {
    fn new(
        pane_kind: ObservationPaneKind,
        harness: HarnessIdentity,
        ground_truth_source: Option<StateSource>,
    ) -> Self {
        Self {
            pane_kind,
            harness,
            ground_truth_source,
            panes: 0,
            observations: 0,
            labels: 0,
            label_transitions: 0,
            labeled_observations: 0,
            panes_with_omitted_sources: 0,
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
        let group_ready = self.label_transitions >= MIN_GROUND_TRUTH_TRANSITIONS;
        self.states.sort_by_key(|(state, _)| state_order(state));
        let states = self
            .states
            .into_iter()
            .map(|(state, mut metrics)| {
                let state_ready =
                    group_ready && metrics.truth_transitions >= MIN_GROUND_TRUTH_TRANSITIONS;
                metrics.latencies.sort_unstable();
                let latency = LatencyMetrics {
                    measured: metrics.latencies.len() as u64,
                    unavailable: metrics.latency_unavailable,
                    median_ms: percentile(&metrics.latencies, 50),
                    p95_ms: percentile(&metrics.latencies, 95),
                    maximum_ms: metrics.latencies.last().copied(),
                };
                metrics.durations.sort_unstable();
                let duration = StateDurationMetrics {
                    measured: metrics.durations.len() as u64,
                    unavailable: metrics.duration_unavailable,
                    minimum_ms: metrics.durations.first().copied(),
                    median_ms: percentile(&metrics.durations, 50),
                    entries_under_one_second: metrics
                        .durations
                        .iter()
                        .filter(|duration| **duration < 1_000)
                        .count() as u64,
                };
                StateMetrics {
                    state,
                    truth_transitions: metrics.truth_transitions,
                    true_positive: metrics.true_positive,
                    false_positive: metrics.false_positive,
                    false_negative: metrics.false_negative,
                    precision: Rate::score(
                        metrics.true_positive,
                        metrics.true_positive.saturating_add(metrics.false_positive),
                        state_ready,
                    ),
                    recall: Rate::score(
                        metrics.true_positive,
                        metrics.true_positive.saturating_add(metrics.false_negative),
                        state_ready,
                    ),
                    time_coverage: self.coverage_available.then(|| {
                        Rate::new(
                            metrics.total_ms.saturating_sub(metrics.lost_ms),
                            metrics.total_ms,
                        )
                    }),
                    latency,
                    duration,
                }
            })
            .collect::<Vec<_>>();
        let scores_available = states
            .iter()
            .any(|state| state.precision.value.is_some() || state.recall.value.is_some());
        HarnessMetrics {
            pane_kind: self.pane_kind,
            harness: self.harness,
            ground_truth_source: self.ground_truth_source,
            panes: self.panes,
            observations: self.observations,
            labels: self.labels,
            label_transitions: self.label_transitions,
            labeled_observations: self.labeled_observations,
            scores_available,
            panes_with_omitted_sources: self.panes_with_omitted_sources,
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
    evaluate_with_omitted_panes(detector, rows, &[])
}

/// Scores a detector while retaining explicit knowledge that one or more
/// panes have sources the current reader omitted. Omitted sources have no
/// trustworthy timestamps, so their groups lose elapsed coverage. Precision
/// and recall remain available over retained labels, marked as partial by
/// `panes_with_omitted_sources`.
pub fn evaluate_with_omitted_panes(
    detector: &dyn Detector,
    rows: &[RecordedRow],
    omitted_pane_ids: &[String],
) -> Result<MetricsReport> {
    let omitted_pane_ids = omitted_pane_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut traces: Vec<PaneTrace> = Vec::new();
    for row in rows {
        row.validate()?;
        let (pane_id, pane_kind, harness, sequence) = identity(row);
        let index = traces
            .iter()
            .position(|trace| {
                trace.pane_id == pane_id
                    && trace.pane_kind == pane_kind
                    && trace.harness == *harness
            })
            .unwrap_or_else(|| {
                traces.push(PaneTrace {
                    pane_id: pane_id.into(),
                    pane_kind,
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
        let sources = label_sources(&trace);
        for source in sources {
            let index = harnesses
                .iter()
                .position(|metrics| {
                    metrics.pane_kind == trace.pane_kind
                        && metrics.harness == trace.harness
                        && metrics.ground_truth_source == source
                })
                .unwrap_or_else(|| {
                    harnesses.push(MutableHarness::new(
                        trace.pane_kind,
                        trace.harness.clone(),
                        source.clone(),
                    ));
                    harnesses.len() - 1
                });
            let metrics = &mut harnesses[index];
            if omitted_pane_ids.contains(trace.pane_id.as_str()) {
                metrics.panes_with_omitted_sources += 1;
                metrics.coverage_available = false;
            }
            score_trace(detector, &trace, source.as_ref(), metrics);
        }
    }
    harnesses.sort_by(|left, right| {
        pane_kind_order(left.pane_kind)
            .cmp(&pane_kind_order(right.pane_kind))
            .then_with(|| harness_name(&left.harness).cmp(harness_name(&right.harness)))
            .then_with(|| {
                source_order(left.ground_truth_source.as_ref())
                    .cmp(&source_order(right.ground_truth_source.as_ref()))
            })
    });
    Ok(MetricsReport {
        harnesses: harnesses.into_iter().map(MutableHarness::finish).collect(),
    })
}

fn score_trace(
    detector: &dyn Detector,
    trace: &PaneTrace,
    ground_truth_source: Option<&StateSource>,
    metrics: &mut MutableHarness,
) {
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
            RecordedRow::Label(label)
                if ground_truth_source.is_some_and(|source| source == &label.source) =>
            {
                Some((row.sequence, label))
            }
            _ => None,
        })
        .collect();
    metrics.observations += observations.len() as u64;
    metrics.labels += labels.len() as u64;
    for pair in labels.windows(2) {
        if pair[0].1.state == pair[1].1.state {
            continue;
        }
        metrics.label_transitions += 1;
        metrics.state_mut(&pair[1].1.state).truth_transitions += 1;
    }
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
    score_state_durations(&labels, metrics);
    score_coverage(trace, &labels, metrics);
}

fn label_sources(trace: &PaneTrace) -> Vec<Option<StateSource>> {
    let mut sources = Vec::new();
    for row in &trace.rows {
        let RecordedRow::Label(label) = &row.row else {
            continue;
        };
        if !sources.iter().any(|source| source == &label.source) {
            sources.push(label.source.clone());
        }
    }
    if sources.is_empty() {
        return vec![None];
    }
    sources.sort_by_key(source_order_value);
    sources.into_iter().map(Some).collect()
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

fn score_state_durations(labels: &[(u64, &crate::GroundTruthLabel)], metrics: &mut MutableHarness) {
    let Some((_, first)) = labels.first() else {
        return;
    };
    let mut current = *first;
    for (_, next) in labels.iter().skip(1) {
        if next.state == current.state {
            continue;
        }
        let state = metrics.state_mut(&current.state);
        if next.timestamp_ms < current.timestamp_ms {
            state.duration_unavailable += 1;
        } else {
            state
                .durations
                .push((next.timestamp_ms - current.timestamp_ms) as u64);
        }
        current = next;
    }
    metrics.state_mut(&current.state).duration_unavailable += 1;
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
            // The label before the gap owns this interval in the ordinary
            // piecewise timeline. Once the labels around the gap disagree,
            // however, assigning that missing interval to either state would
            // guess. Remove it from the per-state denominator and retain it
            // only as harness-level unclassified coverage loss.
            if let Some(before) = before {
                let state = metrics.state_mut(before);
                state.total_ms = state.total_ms.saturating_sub(duration);
            }
        }
    }
}

fn gap_duration(gap: &ObservationGap) -> Option<u64> {
    (gap.last_timestamp_ms >= gap.first_timestamp_ms)
        .then(|| (gap.last_timestamp_ms - gap.first_timestamp_ms) as u64)
}

fn identity(row: &RecordedRow) -> (&str, ObservationPaneKind, &HarnessIdentity, u64) {
    match row {
        RecordedRow::Observation(frame) => (
            &frame.pane_id,
            frame.pane_kind,
            &frame.harness,
            frame.sequence,
        ),
        RecordedRow::Label(label) => (
            &label.pane_id,
            label.pane_kind,
            &label.harness,
            label.sequence,
        ),
        RecordedRow::Gap(gap) => (&gap.pane_id, gap.pane_kind, &gap.harness, gap.last_sequence),
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

fn pane_kind_order(kind: ObservationPaneKind) -> u8 {
    match kind {
        ObservationPaneKind::Agent => 0,
        ObservationPaneKind::Terminal => 1,
    }
}

fn source_order(source: Option<&StateSource>) -> u8 {
    source.map_or(0, source_order_value)
}

fn source_order_value(source: &StateSource) -> u8 {
    match source {
        StateSource::Protocol => 1,
        StateSource::Hook => 2,
        StateSource::Osc => 3,
        StateSource::Screen => 4,
        StateSource::None => 5,
    }
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

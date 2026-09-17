use crate::{
    DATASET_SCHEMA_VERSION, Detector, EngineBuild, Error, HarnessIdentity, MetricsReport,
    ObservationPaneKind, RecordedRow, RegexBaseline, Result, evaluate_with_omitted_panes,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DATASET_FINGERPRINT_DOMAIN: &str = "soyeht-pane-state-evaluation-v1";
const MAX_DETECTOR_NAME_BYTES: usize = 64;

#[derive(Debug, Clone)]
pub struct EvaluationDataset {
    fingerprint_sha256: String,
    sources: Vec<EvaluationSource>,
    engine_builds: Vec<EngineBuild>,
    rows: Vec<RecordedRow>,
    omitted_sources: Vec<EvaluationOmittedSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EvaluationSource {
    pub pane_id: String,
    pub pane_kind: ObservationPaneKind,
    pub harness: HarnessIdentity,
    pub engine: EngineBuild,
    pub segment_sequence: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub clean_close: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EvaluationOmittedSource {
    pub pane_id: String,
    pub schema_version: u32,
    pub segments: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FingerprintInput<'a> {
    domain: &'static str,
    dataset_schema_version: u32,
    sources: &'a [EvaluationSource],
    omitted_sources: &'a [EvaluationOmittedSource],
    rows: &'a [RecordedRow],
}

impl EvaluationDataset {
    pub fn new(
        mut sources: Vec<EvaluationSource>,
        rows: Vec<RecordedRow>,
        mut omitted_sources: Vec<EvaluationOmittedSource>,
    ) -> Result<Self> {
        if sources.is_empty() {
            return Err(Error::Empty("evaluation sources"));
        }
        for source in &sources {
            source.validate()?;
        }
        for row in &rows {
            row.validate()?;
        }
        for omitted in &omitted_sources {
            omitted.validate()?;
        }
        sources.sort_by(|left, right| {
            (
                &left.pane_id,
                left.segment_sequence,
                left.first_sequence,
                left.last_sequence,
            )
                .cmp(&(
                    &right.pane_id,
                    right.segment_sequence,
                    right.first_sequence,
                    right.last_sequence,
                ))
        });
        omitted_sources.sort_by(|left, right| {
            (&left.pane_id, left.schema_version, left.segments).cmp(&(
                &right.pane_id,
                right.schema_version,
                right.segments,
            ))
        });
        validate_source_coverage(&sources, &rows)?;
        let mut engine_builds = sources
            .iter()
            .map(|source| source.engine.clone())
            .collect::<Vec<_>>();
        engine_builds.sort_by(|left, right| {
            (&left.package_version, &left.executable_sha256)
                .cmp(&(&right.package_version, &right.executable_sha256))
        });
        engine_builds.dedup();
        let encoded = serde_json::to_vec(&FingerprintInput {
            domain: DATASET_FINGERPRINT_DOMAIN,
            dataset_schema_version: DATASET_SCHEMA_VERSION,
            sources: &sources,
            omitted_sources: &omitted_sources,
            rows: &rows,
        })
        .map_err(|_| Error::Json)?;
        let fingerprint_sha256 = format!("{:x}", Sha256::digest(encoded));
        Ok(Self {
            fingerprint_sha256,
            sources,
            engine_builds,
            rows,
            omitted_sources,
        })
    }

    pub fn fingerprint_sha256(&self) -> &str {
        &self.fingerprint_sha256
    }

    pub fn engine_builds(&self) -> &[EngineBuild] {
        &self.engine_builds
    }

    pub fn sources(&self) -> &[EvaluationSource] {
        &self.sources
    }

    pub fn rows(&self) -> &[RecordedRow] {
        &self.rows
    }

    pub fn omitted_sources(&self) -> &[EvaluationOmittedSource] {
        &self.omitted_sources
    }

    fn omitted_pane_ids(&self) -> Vec<String> {
        let mut pane_ids = self
            .omitted_sources
            .iter()
            .map(|source| source.pane_id.clone())
            .collect::<Vec<_>>();
        pane_ids.sort();
        pane_ids.dedup();
        pane_ids
    }
}

impl EvaluationSource {
    fn validate(&self) -> Result<()> {
        crate::model::bounded_nonempty(&self.pane_id, crate::MAX_PANE_ID_BYTES, "source pane id")?;
        self.harness.validate()?;
        self.engine.validate()?;
        if self.last_sequence < self.first_sequence {
            return Err(Error::InvalidSequence);
        }
        Ok(())
    }

    fn contains(&self, row: &RecordedRow) -> bool {
        let (pane_id, pane_kind, harness, first_sequence, last_sequence) = row_provenance(row);
        self.pane_id == pane_id
            && self.pane_kind == pane_kind
            && &self.harness == harness
            && self.first_sequence <= first_sequence
            && self.last_sequence >= last_sequence
    }
}

impl EvaluationOmittedSource {
    fn validate(&self) -> Result<()> {
        crate::model::bounded_nonempty(
            &self.pane_id,
            crate::MAX_PANE_ID_BYTES,
            "omitted source pane id",
        )?;
        if self.schema_version == 0 || self.segments == 0 {
            return Err(Error::SchemaVersion);
        }
        Ok(())
    }
}

fn validate_source_coverage(sources: &[EvaluationSource], rows: &[RecordedRow]) -> Result<()> {
    if rows
        .iter()
        .any(|row| sources.iter().filter(|source| source.contains(row)).count() != 1)
        || sources
            .iter()
            .any(|source| !rows.iter().any(|row| source.contains(row)))
    {
        return Err(Error::MismatchedProvenance);
    }
    Ok(())
}

fn row_provenance(row: &RecordedRow) -> (&str, ObservationPaneKind, &HarnessIdentity, u64, u64) {
    match row {
        RecordedRow::Observation(frame) => (
            &frame.pane_id,
            frame.pane_kind,
            &frame.harness,
            frame.sequence,
            frame.sequence,
        ),
        RecordedRow::Label(label) => (
            &label.pane_id,
            label.pane_kind,
            &label.harness,
            label.sequence,
            label.sequence,
        ),
        RecordedRow::Gap(gap) => (
            &gap.pane_id,
            gap.pane_kind,
            &gap.harness,
            gap.first_sequence,
            gap.last_sequence,
        ),
    }
}

pub struct DetectorRegistry {
    detectors: Vec<RegisteredDetector>,
}

struct RegisteredDetector {
    name: String,
    detector: Box<dyn Detector>,
}

impl DetectorRegistry {
    pub fn new() -> Self {
        Self {
            detectors: Vec::new(),
        }
    }

    pub fn shipped() -> Result<Self> {
        Self::new().with("regex_baseline", RegexBaseline::new()?)
    }

    pub fn with(
        mut self,
        name: impl Into<String>,
        detector: impl Detector + 'static,
    ) -> Result<Self> {
        let name = name.into();
        validate_detector_name(&name)?;
        if self.detectors.iter().any(|entry| entry.name == name) {
            return Err(Error::DuplicateDetectorName);
        }
        self.detectors.push(RegisteredDetector {
            name,
            detector: Box::new(detector),
        });
        Ok(self)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.detectors.iter().map(|entry| entry.name.as_str())
    }

    pub fn evaluate(&self, dataset: &EvaluationDataset) -> Result<EvaluationReport> {
        if self.detectors.is_empty() {
            return Err(Error::Empty("detector registry"));
        }
        let detectors = self
            .detectors
            .iter()
            .map(|entry| {
                Ok(DetectorEvaluation {
                    name: entry.name.clone(),
                    metrics: evaluate_with_omitted_panes(
                        entry.detector.as_ref(),
                        dataset.rows(),
                        &dataset.omitted_pane_ids(),
                    )?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(EvaluationReport {
            dataset_fingerprint_sha256: dataset.fingerprint_sha256.clone(),
            dataset_schema_version: DATASET_SCHEMA_VERSION,
            engine_builds: dataset.engine_builds.clone(),
            sources: dataset.sources.clone(),
            omitted_sources: dataset.omitted_sources.clone(),
            counts: DatasetCounts::from_dataset(dataset),
            detectors,
        })
    }
}

impl Default for DetectorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn evaluate_shipped(dataset: &EvaluationDataset) -> Result<EvaluationReport> {
    DetectorRegistry::shipped()?.evaluate(dataset)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetCounts {
    pub rows: u64,
    pub observations: u64,
    pub labels: u64,
    pub gaps: u64,
    pub omitted_panes: u64,
}

impl DatasetCounts {
    fn from_dataset(dataset: &EvaluationDataset) -> Self {
        let mut observations = 0;
        let mut labels = 0;
        let mut gaps = 0;
        for row in dataset.rows() {
            match row {
                RecordedRow::Observation(_) => observations += 1,
                RecordedRow::Label(_) => labels += 1,
                RecordedRow::Gap(_) => gaps += 1,
            }
        }
        Self {
            rows: dataset.rows().len() as u64,
            observations,
            labels,
            gaps,
            omitted_panes: dataset.omitted_pane_ids().len() as u64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectorEvaluation {
    pub name: String,
    pub metrics: MetricsReport,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationReport {
    pub dataset_fingerprint_sha256: String,
    pub dataset_schema_version: u32,
    pub engine_builds: Vec<EngineBuild>,
    pub sources: Vec<EvaluationSource>,
    pub omitted_sources: Vec<EvaluationOmittedSource>,
    pub counts: DatasetCounts,
    pub detectors: Vec<DetectorEvaluation>,
}

impl EvaluationReport {
    pub fn same_dataset_as(&self, other: &Self) -> bool {
        self.dataset_fingerprint_sha256 == other.dataset_fingerprint_sha256
            && self.dataset_schema_version == other.dataset_schema_version
            && self.engine_builds == other.engine_builds
            && self.counts == other.counts
    }
}

fn validate_detector_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= MAX_DETECTOR_NAME_BYTES
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit() && index > 0
                || byte == b'_' && index > 0 && index + 1 < name.len()
        })
        && !name.as_bytes().windows(2).any(|pair| pair == b"__");
    if !valid {
        return Err(Error::InvalidDetectorName);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_names_are_stable_identifiers() {
        for valid in ["regex_baseline", "activity2", "a"] {
            assert!(validate_detector_name(valid).is_ok(), "rejected {valid}");
        }
        for invalid in ["", "Regex", "2activity", "activity-2", "a__b", "a_"] {
            assert!(
                validate_detector_name(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn registry_rejects_duplicate_names_and_empty_evaluation() {
        struct Unknown;
        impl Detector for Unknown {
            fn state(&self, _: &crate::ObservationFrame) -> crate::Detection {
                crate::Detection::unknown()
            }
        }

        assert_eq!(
            DetectorRegistry::new()
                .with("unknown", Unknown)
                .and_then(|registry| registry.with("unknown", Unknown))
                .err(),
            Some(Error::DuplicateDetectorName)
        );
    }
}

use detector::{LogLine, ModelResult, Prediction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelResponse {
    pub score: f32,
    pub threshold: f32,
    pub vote: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FindingResponse {
    pub grid: [usize; 2],
    pub map: Vec<f32>,
    pub area_pct: f32,
    pub severity: String,
    pub defect_probability: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShiftResponse {
    pub distance: f32,
    pub limit: f32,
    pub warning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogResponse {
    pub message: String,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PredictionResponse {
    pub verdict: String,
    pub needs_review: bool,
    pub global: ModelResponse,
    pub patch: Option<ModelResponse>,
    pub finding: Option<FindingResponse>,
    pub shift: ShiftResponse,
    pub log: Vec<LogResponse>,
}

impl From<ModelResult> for ModelResponse {
    fn from(m: ModelResult) -> Self {
        Self {
            score: m.score,
            threshold: m.threshold,
            vote: m.vote.to_string(),
        }
    }
}

impl From<LogLine> for LogResponse {
    fn from(line: LogLine) -> Self {
        Self {
            message: line.message,
            ms: line.ms,
        }
    }
}

impl From<Prediction> for PredictionResponse {
    fn from(p: Prediction) -> Self {
        Self {
            verdict: p.verdict.to_string(),
            needs_review: p.needs_review,
            global: p.global.into(),
            patch: p.patch.map(Into::into),
            finding: p.finding.map(|f| FindingResponse {
                grid: [f.grid.0, f.grid.1],
                map: f.map,
                area_pct: f.area_pct,
                severity: f.severity.to_string(),
                defect_probability: f.defect_probability,
            }),
            shift: ShiftResponse {
                distance: p.shift.distance,
                limit: p.shift.limit,
                warning: p.shift.warning,
            },
            log: p.log.into_iter().map(Into::into).collect(),
        }
    }
}

use std::path::PathBuf;
use std::sync::Arc;

use axum::http::StatusCode;
use detector::{ModelInfo, Prediction, Predictor};
use image::DynamicImage;

use crate::batch::Batches;

/// Anything that can judge an image; lets tests run without model weights.
pub trait Model: Send + Sync + 'static {
    fn predict(&self, image: &DynamicImage) -> Result<Prediction, String>;
    fn info(&self) -> ModelInfo;
}

impl Model for Predictor {
    fn predict(&self, image: &DynamicImage) -> Result<Prediction, String> {
        Predictor::predict(self, image).map_err(|e| e.to_string())
    }

    fn info(&self) -> ModelInfo {
        Predictor::info(self)
    }
}

pub struct AppState {
    pub model: Box<dyn Model>,
    pub decisions_path: PathBuf,
    pub batches: Batches,
    pub min_mean_brightness: f32,
}

impl AppState {
    /// Why an image should not be judged at all, if there is such a reason.
    pub fn unusable(&self, image: &DynamicImage) -> Option<String> {
        (data::mean_brightness(image) < self.min_mean_brightness).then(|| {
            "image is almost black, so it was not judged (is it a mask file rather than a photo?)"
                .to_string()
        })
    }
}

pub type Shared = Arc<AppState>;
pub type ApiError = (StatusCode, String);

pub fn bad_request(message: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, message.to_string())
}

pub fn not_found(message: impl std::fmt::Display) -> ApiError {
    (StatusCode::NOT_FOUND, message.to_string())
}

pub fn internal(message: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, message.to_string())
}

#[cfg(test)]
pub mod testing {
    use analysis::{Severity, Shift};
    use detector::{Finding, LogLine, ModelResult, Verdict};

    use super::*;

    /// Always answers Defect with a small heatmap.
    pub struct StubModel;

    impl Model for StubModel {
        fn predict(&self, _image: &DynamicImage) -> Result<Prediction, String> {
            let model = ModelResult {
                score: 7.0,
                threshold: 5.0,
                vote: Verdict::Defect,
            };
            Ok(Prediction {
                verdict: Verdict::Defect,
                needs_review: false,
                global: model,
                patch: Some(model),
                finding: Some(Finding {
                    map: vec![1.0, 9.0],
                    grid: (1, 2),
                    area_pct: 50.0,
                    severity: Severity::Medium,
                    defect_probability: 0.9,
                }),
                shift: Shift {
                    distance: 1.0,
                    limit: 2.0,
                    warning: false,
                },
                log: vec![LogLine {
                    message: "stub step".to_string(),
                    ms: 1,
                }],
            })
        }

        fn info(&self) -> ModelInfo {
            ModelInfo {
                good_vectors: 3,
                patch_rows: 4,
                global_threshold: 5.0,
                patch_threshold: 6.0,
            }
        }
    }
}

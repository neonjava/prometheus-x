use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::http::StatusCode;
use detector::{Catalog, ModelInfo, Outcome};
use image::DynamicImage;

use crate::batch::Batches;
use crate::camera::Live;

/// Anything that can judge an image; lets tests run without model weights.
pub trait Model: Send + Sync + 'static {
    /// `product` is None to detect the product from the photo.
    fn predict(&self, image: &DynamicImage, product: Option<&str>) -> Result<Outcome, String>;
    fn products(&self) -> Vec<(String, ModelInfo)>;
    /// Learns a camera product from the frames saved in `frames_dir`.
    fn learn(&self, name: &str, frames_dir: &Path) -> Result<ModelInfo, String>;
}

impl Model for Catalog {
    fn predict(&self, image: &DynamicImage, product: Option<&str>) -> Result<Outcome, String> {
        Catalog::predict(self, image, product).map_err(|e| e.to_string())
    }

    fn products(&self) -> Vec<(String, ModelInfo)> {
        Catalog::products(self)
    }

    fn learn(&self, name: &str, frames_dir: &Path) -> Result<ModelInfo, String> {
        Catalog::learn(self, name, frames_dir).map_err(|e| e.to_string())
    }
}

/// The form value "auto" or nothing means: detect the product.
pub fn product_choice(text: String) -> Option<String> {
    Some(text).filter(|name| !name.is_empty() && name != "auto")
}

pub struct AppState {
    pub model: Box<dyn Model>,
    pub decisions_path: PathBuf,
    pub batches: Batches,
    pub min_mean_brightness: f32,
    pub camera: CameraSettings,
    pub live: Live,
}

pub struct CameraSettings {
    pub dir: PathBuf,
    pub min_frames: usize,
    pub val_every: usize,
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
    use detector::{Finding, LogLine, ModelResult, Prediction, Verdict};

    use super::*;

    pub fn camera_settings(dir: &Path) -> CameraSettings {
        CameraSettings {
            dir: dir.to_path_buf(),
            min_frames: 4,
            val_every: 2,
        }
    }

    /// Always answers Defect with a small heatmap.
    pub struct StubModel;

    impl Model for StubModel {
        fn predict(&self, _image: &DynamicImage, product: Option<&str>) -> Result<Outcome, String> {
            let model = ModelResult {
                score: 7.0,
                threshold: 5.0,
                vote: Verdict::Defect,
            };
            let prediction = Prediction {
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
            };
            Ok(Outcome::Judged {
                product: product.unwrap_or("stub").to_string(),
                prediction,
            })
        }

        fn products(&self) -> Vec<(String, ModelInfo)> {
            let info = ModelInfo {
                good_vectors: 3,
                patch_rows: 4,
                global_threshold: 5.0,
                patch_threshold: 6.0,
                camera: false,
            };
            vec![("stub".to_string(), info)]
        }

        fn learn(&self, _name: &str, frames_dir: &Path) -> Result<ModelInfo, String> {
            let count = |sub: &str| std::fs::read_dir(frames_dir.join(sub)).map(Iterator::count);
            Ok(ModelInfo {
                good_vectors: count("train/good").map_err(|e| e.to_string())?,
                patch_rows: count("val/good").map_err(|e| e.to_string())?,
                global_threshold: 1.0,
                patch_threshold: 1.0,
                camera: true,
            })
        }
    }
}

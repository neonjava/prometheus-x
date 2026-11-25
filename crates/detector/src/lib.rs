pub mod patch;
pub mod predict;
pub mod score;
pub mod store;
pub mod verdict;

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DetectorError {
    #[error("no good vectors to compare against")]
    EmptyReference,
    #[error("vector length {got} does not match reference length {expected}")]
    DimensionMismatch { expected: usize, got: usize },
    #[error("no images found in {0}")]
    NoImages(PathBuf),
    #[error("failed to read image {path}: {source}")]
    Image {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid reference file {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("patch bank file {0} has no bank tensor")]
    BadBank(PathBuf),
    #[error(transparent)]
    Candle(#[from] candle_core::Error),
    #[error(transparent)]
    Features(#[from] features::FeaturesError),
    #[error(transparent)]
    Data(#[from] data::DataError),
    #[error(transparent)]
    Analysis(#[from] analysis::AnalysisError),
}

pub use predict::{predict, Finding, LogLine, ModelInfo, ModelResult, Prediction, Predictor};
pub use score::global_score;
pub use verdict::{fuse, verdict, Verdict};

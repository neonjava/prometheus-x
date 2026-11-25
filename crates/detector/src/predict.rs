use std::path::Path;

use config::Config;
use features::Extractor;
use image::DynamicImage;

use crate::store::{open_image, Reference};
use crate::{global_score, verdict, DetectorError, Verdict};

#[derive(Debug, Clone, Copy)]
pub struct Prediction {
    pub verdict: Verdict,
    pub score: f32,
}

/// Loaded model: keeps the extractor and reference in memory between predictions.
pub struct Predictor {
    extractor: Extractor,
    reference: Reference,
    margin: f32,
}

impl Predictor {
    pub fn load(cfg: &Config) -> Result<Self, DetectorError> {
        Ok(Self {
            extractor: Extractor::load(cfg)?,
            reference: Reference::load(&cfg.good_vectors_path)?,
            margin: cfg.uncertain_margin,
        })
    }

    pub fn threshold(&self) -> f32 {
        self.reference.threshold
    }

    pub fn score(&self, image: &DynamicImage) -> Result<f32, DetectorError> {
        let vector = self.extractor.extract(image)?;
        global_score(&vector, &self.reference.vectors)
    }

    pub fn predict(&self, image: &DynamicImage) -> Result<Prediction, DetectorError> {
        let score = self.score(image)?;
        Ok(Prediction {
            verdict: verdict(score, self.reference.threshold, self.margin),
            score,
        })
    }
}

/// One-shot prediction from a file path. Loads the model every call.
pub fn predict(cfg: &Config, image_path: &Path) -> Result<Prediction, DetectorError> {
    let image = open_image(image_path)?;
    Predictor::load(cfg)?.predict(&image)
}

use std::path::Path;

use config::Config;
use features::Extractor;
use image::DynamicImage;

use crate::patch::PatchBank;
use crate::store::{open_image, Reference};
use crate::{fuse, global_score, verdict, DetectorError, Verdict};

/// One model's opinion on an image.
#[derive(Debug, Clone, Copy)]
pub struct ModelResult {
    pub score: f32,
    pub threshold: f32,
    pub vote: Verdict,
}

#[derive(Debug, Clone)]
pub struct Prediction {
    pub verdict: Verdict,
    /// True when a human should look: the models disagree or a score is near its threshold.
    pub needs_review: bool,
    pub global: ModelResult,
    pub patch: ModelResult,
    /// Distance of every patch to the good bank, row by row over `patch_grid`.
    pub patch_map: Vec<f32>,
    pub patch_grid: (usize, usize),
}

/// Loaded models: kept in memory between predictions.
pub struct Predictor {
    extractor: Extractor,
    reference: Reference,
    bank: PatchBank,
    margin: f32,
}

impl Predictor {
    pub fn load(cfg: &Config) -> Result<Self, DetectorError> {
        let extractor = Extractor::load(cfg)?;
        let bank = PatchBank::load(&cfg.patch_bank_path, extractor.device())?;
        Ok(Self {
            extractor,
            bank,
            reference: Reference::load(&cfg.good_vectors_path)?,
            margin: cfg.uncertain_margin,
        })
    }

    pub fn predict(&self, image: &DynamicImage) -> Result<Prediction, DetectorError> {
        let features = self.extractor.extract_all(image)?;
        let global = self.vote(
            global_score(&features.global, &self.reference.vectors)?,
            self.reference.threshold,
        );
        let patch_score = self.bank.score(&features.patches, features.grid)?;
        let patch = self.vote(patch_score.score, self.reference.patch_threshold);
        let verdict = fuse(global.vote, patch.vote);
        Ok(Prediction {
            verdict,
            needs_review: verdict == Verdict::Uncertain,
            global,
            patch,
            patch_map: patch_score.map,
            patch_grid: patch_score.grid,
        })
    }

    fn vote(&self, score: f32, threshold: f32) -> ModelResult {
        ModelResult {
            score,
            threshold,
            vote: verdict(score, threshold, self.margin),
        }
    }
}

/// One-shot prediction from a file path. Loads the models every call.
pub fn predict(cfg: &Config, image_path: &Path) -> Result<Prediction, DetectorError> {
    let image = open_image(image_path)?;
    Predictor::load(cfg)?.predict(&image)
}

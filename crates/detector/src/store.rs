use std::path::Path;

use candle_core::Tensor;
use config::Config;
use features::Extractor;
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};

use crate::patch::{sample_rows, PatchBank};
use crate::{global_score, DetectorError};

/// What a trained detector needs at prediction time.
#[derive(Debug, Serialize, Deserialize)]
pub struct Reference {
    pub vectors: Vec<Vec<f32>>,
    pub threshold: f32,
    /// Scores of the good validation images; used later for calibration.
    pub val_scores: Vec<f32>,
    pub patch_threshold: f32,
    pub patch_val_scores: Vec<f32>,
    /// True for camera-taught products: a scratch or sticker barely moves the whole-photo
    /// score, so the patch check alone decides.
    #[serde(default)]
    pub patch_decides: bool,
}

impl Reference {
    pub fn save(&self, path: &Path) -> Result<(), DetectorError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|source| io_err(parent, source))?;
        }
        let text = serde_json::to_string(self).map_err(|source| DetectorError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        std::fs::write(path, text).map_err(|source| io_err(path, source))
    }

    pub fn load(path: &Path) -> Result<Self, DetectorError> {
        let text = std::fs::read_to_string(path).map_err(|source| io_err(path, source))?;
        serde_json::from_str(&text).map_err(|source| DetectorError::Json {
            path: path.to_path_buf(),
            source,
        })
    }
}

fn io_err(path: &Path, source: std::io::Error) -> DetectorError {
    DetectorError::Io {
        path: path.to_path_buf(),
        source,
    }
}

pub fn open_image(path: &Path) -> Result<image::DynamicImage, DetectorError> {
    image::open(path).map_err(|source| DetectorError::Image {
        path: path.to_path_buf(),
        source,
    })
}

/// Builds both models from `train_dir` and calibrates their thresholds on `val_dir`.
pub fn build_reference(
    cfg: &Config,
    extractor: &Extractor,
) -> Result<(Reference, PatchBank), DetectorError> {
    let (vectors, bank) = learn_good(cfg, extractor)?;
    let (val_scores, patch_val_scores) = score_val(cfg, extractor, &vectors, &bank)?;
    let threshold = analysis::pick_threshold(&val_scores, cfg.threshold_percentile)?;
    let patch_threshold = analysis::pick_threshold(&patch_val_scores, cfg.threshold_percentile)?;
    let reference = Reference {
        vectors,
        threshold,
        val_scores,
        patch_threshold,
        patch_val_scores,
        patch_decides: false,
    };
    Ok((reference, bank))
}

fn image_paths(dir: &Path) -> Result<Vec<std::path::PathBuf>, DetectorError> {
    let paths = data::list_images(dir)?;
    if paths.is_empty() {
        return Err(DetectorError::NoImages(dir.to_path_buf()));
    }
    Ok(paths)
}

// One pass over the good images: keep every global vector, and a random share of patches.
fn learn_good(
    cfg: &Config,
    extractor: &Extractor,
) -> Result<(Vec<Vec<f32>>, PatchBank), DetectorError> {
    let paths = image_paths(&cfg.train_dir)?;
    let per_image = cfg.patch_bank_size.div_ceil(paths.len());
    let mut rng = StdRng::seed_from_u64(cfg.patch_seed);
    let mut vectors = Vec::with_capacity(paths.len());
    let mut samples = Vec::with_capacity(paths.len());
    for path in &paths {
        let features = extractor.extract_all(&open_image(path)?)?;
        samples.push(sample_rows(&features.patches, per_image, &mut rng)?);
        vectors.push(features.global);
    }
    let bank = PatchBank::from_rows(Tensor::cat(&samples, 0)?)?;
    Ok((vectors, bank))
}

fn score_val(
    cfg: &Config,
    extractor: &Extractor,
    vectors: &[Vec<f32>],
    bank: &PatchBank,
) -> Result<(Vec<f32>, Vec<f32>), DetectorError> {
    let mut global = Vec::new();
    let mut patch = Vec::new();
    for path in image_paths(&cfg.val_dir)? {
        let features = extractor.extract_all(&open_image(&path)?)?;
        global.push(global_score(&features.global, vectors)?);
        patch.push(bank.score(&features.patches, features.grid)?.score);
    }
    Ok((global, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_gives_same_reference() {
        let reference = Reference {
            vectors: vec![vec![1.0, 2.0], vec![3.0, 4.0]],
            threshold: 0.5,
            val_scores: vec![0.1, 0.2],
            patch_threshold: 0.7,
            patch_val_scores: vec![0.3],
            patch_decides: true,
        };
        let path = std::env::temp_dir().join(format!("px_ref_{}.json", std::process::id()));
        reference.save(&path).expect("save");
        let loaded = Reference::load(&path).expect("load");
        std::fs::remove_file(&path).expect("cleanup");
        assert_eq!(loaded.vectors, reference.vectors);
        assert_eq!(loaded.threshold, reference.threshold);
        assert_eq!(loaded.patch_threshold, reference.patch_threshold);
        assert!(Reference::load(&path).is_err());
    }
}

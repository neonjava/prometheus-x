use std::path::Path;

use config::Config;
use features::Extractor;
use serde::{Deserialize, Serialize};

use crate::{global_score, DetectorError};

/// What a trained detector needs at prediction time.
#[derive(Debug, Serialize, Deserialize)]
pub struct Reference {
    pub vectors: Vec<Vec<f32>>,
    pub threshold: f32,
    /// Scores of the good validation images; used later for calibration.
    pub val_scores: Vec<f32>,
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

/// Extracts one vector per image in `dir`, loading one image at a time to save memory.
pub fn extract_dir(extractor: &Extractor, dir: &Path) -> Result<Vec<Vec<f32>>, DetectorError> {
    let paths = data::list_images(dir)?;
    if paths.is_empty() {
        return Err(DetectorError::NoImages(dir.to_path_buf()));
    }
    paths
        .iter()
        .map(|path| {
            let image = open_image(path)?;
            Ok(extractor.extract(&image)?)
        })
        .collect()
}

pub fn open_image(path: &Path) -> Result<image::DynamicImage, DetectorError> {
    image::open(path).map_err(|source| DetectorError::Image {
        path: path.to_path_buf(),
        source,
    })
}

/// Builds the reference from `train_dir` and calibrates the threshold on `val_dir`.
pub fn build_reference(cfg: &Config, extractor: &Extractor) -> Result<Reference, DetectorError> {
    let vectors = extract_dir(extractor, &cfg.train_dir)?;
    let val_vectors = extract_dir(extractor, &cfg.val_dir)?;
    let val_scores = val_vectors
        .iter()
        .map(|v| global_score(v, &vectors))
        .collect::<Result<Vec<_>, _>>()?;
    let threshold = analysis::pick_threshold(&val_scores, cfg.threshold_percentile)?;
    Ok(Reference {
        vectors,
        threshold,
        val_scores,
    })
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
        };
        let path = std::env::temp_dir().join(format!("px_ref_{}.json", std::process::id()));
        reference.save(&path).expect("save");
        let loaded = Reference::load(&path).expect("load");
        std::fs::remove_file(&path).expect("cleanup");
        assert_eq!(loaded.vectors, reference.vectors);
        assert_eq!(loaded.threshold, reference.threshold);
        assert!(Reference::load(&path).is_err());
    }
}

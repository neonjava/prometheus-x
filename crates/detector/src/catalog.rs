//! One model per product, one shared feature extractor.
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::sync::Arc;

use config::Config;
use features::Extractor;
use image::DynamicImage;

use crate::predict::{ModelInfo, Trace};
use crate::{DetectorError, LogLine, Prediction, Predictor};

pub struct Catalog {
    extractor: Arc<Extractor>,
    products: BTreeMap<String, Predictor>,
    unknown_ratio: f32,
}

pub enum Outcome {
    Judged {
        product: String,
        prediction: Prediction,
    },
    /// The photo is far from every learned product, so no verdict is given.
    Unknown {
        closest: String,
        score: f32,
        threshold: f32,
        log: Vec<LogLine>,
    },
}

impl Catalog {
    /// Loads the default product plus every trained folder inside `products_dir`.
    pub fn load(cfg: &Config) -> Result<Self, DetectorError> {
        let extractor = Arc::new(Extractor::load(cfg)?);
        let mut products = BTreeMap::new();
        if cfg.good_vectors_path.is_file() && cfg.patch_bank_path.is_file() {
            let predictor = Predictor::load_shared(cfg, extractor.clone())?;
            products.insert(cfg.default_product.clone(), predictor);
        }
        for (name, product_cfg) in product_configs(cfg)? {
            let predictor = Predictor::load_shared(&product_cfg, extractor.clone())?;
            products.insert(name, predictor);
        }
        if products.is_empty() {
            return Err(DetectorError::NoProducts(cfg.products_dir.clone()));
        }
        Ok(Self {
            extractor,
            products,
            unknown_ratio: cfg.unknown_ratio,
        })
    }

    pub fn products(&self) -> Vec<(String, ModelInfo)> {
        self.products
            .iter()
            .map(|(name, predictor)| (name.clone(), predictor.info()))
            .collect()
    }

    /// Judges `image` as `choice`, or detects the product when no choice is given.
    pub fn predict(
        &self,
        image: &DynamicImage,
        choice: Option<&str>,
    ) -> Result<Outcome, DetectorError> {
        let mut trace = Trace::new();
        let features = self.extractor.extract_all(image)?;
        let (name, predictor) = match choice {
            Some(wanted) => self
                .products
                .get_key_value(wanted)
                .ok_or_else(|| DetectorError::UnknownProduct(wanted.to_string()))?,
            None => self.closest(&features.global)?,
        };
        if choice.is_none() {
            let score = predictor.distance(&features.global)?;
            let threshold = predictor.info().global_threshold;
            if is_foreign(score, threshold, self.unknown_ratio) {
                trace.add(format!(
                    "Product check: closest is {name}, but {score:.2} is more than {} x its limit {threshold:.2} -> unknown product, not judged",
                    self.unknown_ratio
                ));
                return Ok(Outcome::Unknown {
                    closest: name.clone(),
                    score,
                    threshold,
                    log: trace.into_lines(),
                });
            }
            trace.add(format!(
                "Product detected: {name} (closest of {})",
                self.products.len()
            ));
        }
        let prediction = predictor.judge(&features, false, trace)?;
        Ok(Outcome::Judged {
            product: name.clone(),
            prediction,
        })
    }

    // The product whose good photos lie nearest to this one.
    fn closest(&self, vector: &[f32]) -> Result<(&String, &Predictor), DetectorError> {
        let mut best: Option<(f32, &String, &Predictor)> = None;
        for (name, predictor) in &self.products {
            let distance = predictor.distance(vector)?;
            if best.is_none_or(|(d, _, _)| distance < d) {
                best = Some((distance, name, predictor));
            }
        }
        best.map(|(_, name, predictor)| (name, predictor))
            .ok_or(DetectorError::EmptyReference)
    }
}

fn is_foreign(score: f32, threshold: f32, ratio: f32) -> bool {
    score > threshold * ratio
}

/// A config per trained folder: same settings, that folder's model files.
fn product_configs(cfg: &Config) -> Result<Vec<(String, Config)>, DetectorError> {
    if !cfg.products_dir.is_dir() {
        return Ok(Vec::new());
    }
    let io_err = |source| DetectorError::Io {
        path: cfg.products_dir.clone(),
        source,
    };
    let (Some(vectors), Some(bank)) = (
        cfg.good_vectors_path.file_name(),
        cfg.patch_bank_path.file_name(),
    ) else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&cfg.products_dir).map_err(io_err)? {
        let dir = entry.map_err(io_err)?.path();
        let mut product = cfg.clone();
        product.good_vectors_path = dir.join(vectors);
        product.patch_bank_path = dir.join(bank);
        // Half-trained folders are skipped until both files exist.
        let trained = product.good_vectors_path.is_file() && product.patch_bank_path.is_file();
        if let Some(name) = dir.file_name().and_then(OsStr::to_str).filter(|_| trained) {
            found.push((name.to_string(), product));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_far_photos_are_foreign() {
        assert!(!is_foreign(5.0, 5.0, 3.0));
        assert!(!is_foreign(15.0, 5.0, 3.0));
        assert!(is_foreign(15.1, 5.0, 3.0));
    }
}

//! One model per product, one shared feature extractor.
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use config::Config;
use features::Extractor;
use image::DynamicImage;

use crate::predict::{ModelInfo, Trace};
use crate::store::build_reference;
use crate::{DetectorError, LogLine, Prediction, Predictor};

pub struct Catalog {
    extractor: Arc<Extractor>,
    /// Behind a lock so a camera product can be added while the server runs.
    products: RwLock<BTreeMap<String, Arc<Predictor>>>,
    learning: Mutex<()>,
    cfg: Config,
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
            products.insert(cfg.default_product.clone(), Arc::new(predictor));
        }
        for (name, product_cfg) in product_configs(cfg)? {
            let predictor = Predictor::load_shared(&product_cfg, extractor.clone())?;
            products.insert(name, Arc::new(predictor));
        }
        if products.is_empty() {
            return Err(DetectorError::NoProducts(cfg.products_dir.clone()));
        }
        Ok(Self {
            extractor,
            products: RwLock::new(products),
            learning: Mutex::new(()),
            cfg: cfg.clone(),
        })
    }

    fn loaded(&self) -> BTreeMap<String, Arc<Predictor>> {
        // A poisoned lock still holds a complete map, so it is safe to read.
        self.products
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn products(&self) -> Vec<(String, ModelInfo)> {
        self.loaded()
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
        let products = self.loaded();
        let (name, predictor) = match choice {
            Some(wanted) => products
                .get_key_value(wanted)
                .ok_or_else(|| DetectorError::UnknownProduct(wanted.to_string()))?,
            None => closest(&products, &features.global)?,
        };
        if choice.is_none() {
            let score = predictor.distance(&features.global)?;
            let threshold = predictor.info().global_threshold;
            if is_foreign(score, threshold, self.cfg.unknown_ratio) {
                trace.add(format!(
                    "Product check: closest is {name}, but {score:.2} is more than {} x its limit {threshold:.2} -> unknown product, not judged",
                    self.cfg.unknown_ratio
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
                products.len()
            ));
        }
        let prediction = predictor.judge(&features, false, trace)?;
        Ok(Outcome::Judged {
            product: name.clone(),
            prediction,
        })
    }

    /// Learns a product from camera frames in `frames_dir` (`train/good`, `val/good`),
    /// saves it under `products_dir/name` and makes it usable at once.
    pub fn learn(&self, name: &str, frames_dir: &Path) -> Result<ModelInfo, DetectorError> {
        let _one_at_a_time = self.learning.try_lock().map_err(|_| DetectorError::Busy)?;
        if self
            .loaded()
            .get(name)
            .is_some_and(|existing| !existing.info().camera)
        {
            return Err(DetectorError::NameTaken(name.to_string()));
        }
        let product = camera_config(&self.cfg, name, frames_dir)?;
        let (mut reference, bank) = build_reference(&product, &self.extractor)?;
        reference.threshold *= self.cfg.camera_threshold_slack;
        reference.patch_threshold *= self.cfg.camera_threshold_slack;
        reference.patch_decides = true;
        // The bank is written first: a product counts as trained only once both files exist.
        bank.save(&product.patch_bank_path)?;
        reference.save(&product.good_vectors_path)?;
        let predictor = Arc::new(Predictor::load_shared(&product, self.extractor.clone())?);
        let info = predictor.info();
        self.products
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_string(), predictor);
        Ok(info)
    }
}

// The product whose good photos lie nearest to this one.
fn closest<'a>(
    products: &'a BTreeMap<String, Arc<Predictor>>,
    vector: &[f32],
) -> Result<(&'a String, &'a Arc<Predictor>), DetectorError> {
    let mut best: Option<(f32, &String, &Arc<Predictor>)> = None;
    for (name, predictor) in products {
        let distance = predictor.distance(vector)?;
        if best.is_none_or(|(d, _, _)| distance < d) {
            best = Some((distance, name, predictor));
        }
    }
    best.map(|(_, name, predictor)| (name, predictor))
        .ok_or(DetectorError::EmptyReference)
}

fn camera_config(cfg: &Config, name: &str, frames_dir: &Path) -> Result<Config, DetectorError> {
    let dir = cfg.products_dir.join(name);
    std::fs::create_dir_all(&dir).map_err(|source| DetectorError::Io {
        path: dir.clone(),
        source,
    })?;
    let mut product = cfg.clone();
    product.train_dir = frames_dir.join("train/good");
    product.val_dir = frames_dir.join("val/good");
    product.good_vectors_path = dir.join(file_name(&cfg.good_vectors_path)?);
    product.patch_bank_path = dir.join(file_name(&cfg.patch_bank_path)?);
    product.threshold_percentile = cfg.camera_threshold_percentile;
    product.patch_bank_size = cfg.camera_patch_bank_size;
    Ok(product)
}

fn file_name(path: &Path) -> Result<&OsStr, DetectorError> {
    path.file_name()
        .ok_or_else(|| DetectorError::NoProducts(path.to_path_buf()))
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

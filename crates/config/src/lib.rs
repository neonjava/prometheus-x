use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

/// A synthetic change to a photo, used to test robustness to lighting and camera changes.
#[derive(Debug, Clone, Deserialize)]
pub struct Perturbation {
    pub name: String,
    pub kind: PerturbationKind,
    pub amount: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PerturbationKind {
    Brightness,
    Contrast,
    Blur,
    Hue,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid config value: {0}")]
    Invalid(String),
}

/// Every tunable value of the project. Keys mirror `config.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub model_weights_path: PathBuf,
    pub input_size: usize,
    pub good_vectors_path: PathBuf,
    pub train_dir: PathBuf,
    pub val_dir: PathBuf,
    pub test_dir: PathBuf,
    /// Percentile (0-100) of good validation scores used as the threshold.
    pub threshold_percentile: f32,
    /// Relative band around the threshold that counts as Uncertain (0.1 = +-10%).
    pub uncertain_margin: f32,
    pub server_bind: String,
    pub max_upload_bytes: usize,
    /// "cpu" or "cuda:<index>".
    pub device: String,
    pub patch_bank_path: PathBuf,
    /// Total patch features kept in the memory bank.
    pub patch_bank_size: usize,
    pub patch_seed: u64,
    /// Percentile (0-100) of training distances to the training mean that triggers a shift warning.
    pub shift_percentile: f32,
    /// Severity index (defect area % x relative score) up to which a defect is Low.
    pub severity_low_max: f32,
    /// Severity index up to which a defect is Medium; above it is High.
    pub severity_med_max: f32,
    /// Human review decisions are appended here, one JSON object per line.
    pub decisions_path: PathBuf,
    /// Folder with the web page files (index.html, css, js).
    pub web_dir: PathBuf,
    /// Uploaded batches, their thumbnails and results are kept here.
    pub batches_dir: PathBuf,
    /// Largest accepted batch upload (zip or many images), in bytes.
    pub max_batch_bytes: u64,
    /// Largest total size a zip may unpack to, in bytes (zip-bomb guard).
    pub max_unpacked_bytes: u64,
    pub max_batch_files: usize,
    /// Images checked at the same time inside one batch.
    pub batch_workers: usize,
    /// Longest side of a batch thumbnail, in pixels.
    pub thumb_size: u32,
    /// Images whose average brightness (0-255) is below this are rejected as blank, not judged.
    pub min_mean_brightness: f32,
    /// Each sub-folder with a trained model is one product.
    pub products_dir: PathBuf,
    /// Name shown for the model at `good_vectors_path`.
    pub default_product: String,
    /// A photo farther than this multiple of the closest product's limit is an unknown product.
    pub unknown_ratio: f32,
    /// Frames captured from the camera are kept here.
    pub captures_dir: PathBuf,
    pub camera_min_frames: usize,
    /// Every n-th camera frame is held out to set the limits.
    pub camera_val_every: usize,
    /// Percentile (0-100) of held-out frame scores used for camera products.
    pub camera_threshold_percentile: f32,
    /// Camera limits are multiplied by this, since a live view varies more than a few frames show.
    pub camera_threshold_slack: f32,
    /// Patches kept for a camera product; fewer means faster live checks.
    pub camera_patch_bank_size: usize,
    /// Frames the camera page records to learn an object.
    pub camera_learn_frames: usize,
    /// Fewest odd patches that count as a live anomaly; smaller spots are glare or blur.
    pub camera_min_spot_cells: usize,
    /// Patches ignored around the live frame, where background objects appear.
    pub camera_edge_cells: usize,
    /// Images per class (good, defect) used by the robustness test.
    pub robustness_sample: usize,
    #[serde(default)]
    pub perturbations: Vec<Perturbation>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let config: Self = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.input_size == 0 {
            return Err(ConfigError::Invalid("input_size must be > 0".into()));
        }
        if !(0.0..=100.0).contains(&self.threshold_percentile) {
            return Err(ConfigError::Invalid(
                "threshold_percentile must be in 0..=100".into(),
            ));
        }
        if !(0.0..1.0).contains(&self.uncertain_margin) {
            return Err(ConfigError::Invalid(
                "uncertain_margin must be in 0..1".into(),
            ));
        }
        if self.max_upload_bytes == 0 {
            return Err(ConfigError::Invalid("max_upload_bytes must be > 0".into()));
        }
        if self.max_batch_bytes == 0
            || self.max_unpacked_bytes == 0
            || self.max_batch_files == 0
            || self.batch_workers == 0
            || self.thumb_size == 0
        {
            return Err(ConfigError::Invalid(
                "batch limits, workers and thumb_size must be > 0".into(),
            ));
        }
        if !(0.0..=255.0).contains(&self.min_mean_brightness) {
            return Err(ConfigError::Invalid(
                "min_mean_brightness must be in 0..=255".into(),
            ));
        }
        if self.unknown_ratio <= 1.0 {
            return Err(ConfigError::Invalid("unknown_ratio must be > 1".into()));
        }
        if self.camera_val_every < 2 || self.camera_min_frames < self.camera_val_every {
            return Err(ConfigError::Invalid(
                "camera_val_every must be >= 2 and <= camera_min_frames".into(),
            ));
        }
        if !(0.0..=100.0).contains(&self.camera_threshold_percentile)
            || self.camera_threshold_slack < 1.0
            || self.camera_patch_bank_size == 0
        {
            return Err(ConfigError::Invalid(
                "camera_threshold_percentile must be 0..=100 and camera_threshold_slack >= 1"
                    .into(),
            ));
        }
        if self.camera_learn_frames < self.camera_min_frames || self.camera_min_spot_cells == 0 {
            return Err(ConfigError::Invalid(
                "camera_learn_frames must be >= camera_min_frames and camera_min_spot_cells > 0"
                    .into(),
            ));
        }
        if self.patch_bank_size == 0 {
            return Err(ConfigError::Invalid("patch_bank_size must be > 0".into()));
        }
        if !(0.0..=100.0).contains(&self.shift_percentile) {
            return Err(ConfigError::Invalid(
                "shift_percentile must be in 0..=100".into(),
            ));
        }
        if self.severity_low_max >= self.severity_med_max {
            return Err(ConfigError::Invalid(
                "severity_low_max must be below severity_med_max".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
model_weights_path = "m.safetensors"
input_size = 224
good_vectors_path = "g.json"
train_dir = "tr"
val_dir = "va"
test_dir = "te"
threshold_percentile = 95.0
uncertain_margin = 0.1
server_bind = "127.0.0.1:0"
max_upload_bytes = 1024
device = "cpu"
patch_bank_path = "b.safetensors"
patch_bank_size = 100
patch_seed = 1
shift_percentile = 99.0
severity_low_max = 5.0
severity_med_max = 15.0
decisions_path = "d.jsonl"
web_dir = "web"
batches_dir = "data/batches"
max_batch_bytes = 1000
max_unpacked_bytes = 1000
max_batch_files = 10
batch_workers = 1
thumb_size = 64
min_mean_brightness = 10.0
products_dir = "p"
default_product = "x"
unknown_ratio = 3.0
captures_dir = "c"
camera_min_frames = 10
camera_val_every = 5
camera_threshold_percentile = 100.0
camera_threshold_slack = 1.2
camera_patch_bank_size = 100
camera_learn_frames = 20
camera_min_spot_cells = 1
camera_edge_cells = 0
robustness_sample = 5

[[perturbations]]
name = "darker"
kind = "brightness"
amount = -40.0
"#;

    fn write(name: &str, text: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("px_config_{name}_{}.toml", std::process::id()));
        std::fs::write(&path, text).expect("write temp config");
        path
    }

    #[test]
    fn loads_valid_config() {
        let path = write("ok", VALID);
        let cfg = Config::load(&path).expect("valid config");
        assert_eq!(cfg.input_size, 224);
        assert_eq!(cfg.server_bind, "127.0.0.1:0");
        assert_eq!(cfg.perturbations.len(), 1);
        assert_eq!(cfg.perturbations[0].kind, PerturbationKind::Brightness);
    }

    #[test]
    fn rejects_bad_values_and_missing_files() {
        let path = write("bad", &VALID.replace("input_size = 224", "input_size = 0"));
        assert!(matches!(Config::load(&path), Err(ConfigError::Invalid(_))));
        let path = write("pct", &VALID.replace("95.0", "150.0"));
        assert!(matches!(Config::load(&path), Err(ConfigError::Invalid(_))));
        assert!(matches!(
            Config::load(Path::new("missing_px_config.toml")),
            Err(ConfigError::Io { .. })
        ));
    }
}

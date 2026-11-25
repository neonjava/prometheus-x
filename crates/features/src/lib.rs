use std::path::PathBuf;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use config::Config;
use image::{imageops::FilterType, DynamicImage};
use thiserror::Error;

mod trunk;
use trunk::{Stages, Trunk};

const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];
const PIXEL_SCALE: f64 = 255.0;

#[derive(Debug, Error)]
pub enum FeaturesError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("failed to read weights from {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("candle error: {0}")]
    Candle(#[from] candle_core::Error),
}

pub struct Extractor {
    model: Trunk,
    input_size: usize,
    device: Device,
}

impl Extractor {
    pub fn load(config: &Config) -> Result<Self, FeaturesError> {
        if config.input_size == 0 {
            return Err(FeaturesError::InvalidConfig(
                "input_size must be greater than 0".to_string(),
            ));
        }
        if config.model_weights_path.as_os_str().is_empty() {
            return Err(FeaturesError::InvalidConfig(
                "model_weights_path must not be empty".to_string(),
            ));
        }

        let device = Device::Cpu;
        let bytes =
            std::fs::read(&config.model_weights_path).map_err(|source| FeaturesError::Io {
                path: config.model_weights_path.clone(),
                source,
            })?;
        let vb = VarBuilder::from_buffered_safetensors(bytes, DType::F32, &device)?;
        let model = Trunk::load(vb)?;

        Ok(Self {
            model,
            input_size: config.input_size,
            device,
        })
    }

    pub fn extract(&self, image: &DynamicImage) -> Result<Vec<f32>, FeaturesError> {
        Ok(self.stages(image)?.pooled.squeeze(0)?.to_vec1::<f32>()?)
    }

    /// Global vector plus one feature row per image patch, from a single forward pass.
    pub fn extract_all(&self, image: &DynamicImage) -> Result<ImageFeatures, FeaturesError> {
        let stages = self.stages(image)?;
        let (patches, grid) = patch_rows(&stages)?;
        Ok(ImageFeatures {
            global: stages.pooled.squeeze(0)?.to_vec1::<f32>()?,
            patches,
            grid,
        })
    }

    fn stages(&self, image: &DynamicImage) -> Result<Stages, FeaturesError> {
        let input = preprocess(image, self.input_size, &self.device)?;
        Ok(self.model.forward(&input)?)
    }
}

/// Features of one image: a global vector and a `(h * w, channels)` patch matrix.
pub struct ImageFeatures {
    pub global: Vec<f32>,
    pub patches: Tensor,
    pub grid: (usize, usize),
}

// layer3 is upsampled to layer2's grid and stacked, so each patch sees both scales.
fn patch_rows(stages: &Stages) -> Result<(Tensor, (usize, usize)), FeaturesError> {
    let (_, _, h, w) = stages.layer2.dims4()?;
    let layer3 = stages.layer3.upsample_nearest2d(h, w)?;
    let channels = Tensor::cat(&[&stages.layer2, &layer3], 1)?.squeeze(0)?;
    let depth = channels.dim(0)?;
    let rows = channels.reshape((depth, h * w))?.t()?.contiguous()?;
    Ok((rows, (h, w)))
}

fn preprocess(
    image: &DynamicImage,
    input_size: usize,
    device: &Device,
) -> Result<Tensor, FeaturesError> {
    let size_u32 = u32::try_from(input_size).map_err(|_| {
        FeaturesError::InvalidConfig(format!("input_size {input_size} exceeds u32"))
    })?;
    // Resize without cropping so edge defects stay in frame.
    let rgb = image
        .resize_exact(size_u32, size_u32, FilterType::Triangle)
        .to_rgb8();

    let tensor = Tensor::from_vec(rgb.into_raw(), (input_size, input_size, 3), device)?
        .permute((2, 0, 1))?;
    let mean = Tensor::new(&IMAGENET_MEAN, device)?.reshape((3, 1, 1))?;
    let std = Tensor::new(&IMAGENET_STD, device)?.reshape((3, 1, 1))?;

    let normalized = (tensor.to_dtype(DType::F32)? / PIXEL_SCALE)?
        .broadcast_sub(&mean)?
        .broadcast_div(&std)?;
    Ok(normalized.unsqueeze(0)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_nn::Module;
    use candle_nn::VarMap;
    use image::{Rgb, RgbImage};
    use std::path::Path;

    struct TempFile(PathBuf);

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn write_test_weights(path: &Path) {
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, &Device::Cpu);
        let _model = candle_transformers::models::resnet::resnet18_no_final_layer(vb)
            .expect("build resnet18 graph");
        varmap.save(path).expect("save test safetensors");
    }

    fn test_config(model_weights_path: PathBuf, input_size: usize) -> Config {
        Config {
            model_weights_path,
            input_size,
            good_vectors_path: PathBuf::new(),
            train_dir: PathBuf::new(),
            val_dir: PathBuf::new(),
            test_dir: PathBuf::new(),
            threshold_percentile: 95.0,
            uncertain_margin: 0.1,
            server_bind: String::new(),
            max_upload_bytes: 1,
        }
    }

    fn synthetic_image(offset: u8) -> DynamicImage {
        let img = RgbImage::from_fn(48, 48, |x, y| {
            Rgb([
                (x as u8).wrapping_add(offset),
                (y as u8).wrapping_add(offset),
                offset,
            ])
        });
        DynamicImage::ImageRgb8(img)
    }

    fn temp_weights_path(name: &str) -> TempFile {
        let path = std::env::temp_dir().join(format!(
            "prometheus_x_features_{name}_{}.safetensors",
            std::process::id()
        ));
        write_test_weights(&path);
        TempFile(path)
    }

    #[test]
    fn same_image_gives_same_fixed_length_vector() {
        let weights = temp_weights_path("same");
        let cfg = test_config(weights.0.clone(), 32);
        let extractor = Extractor::load(&cfg).expect("load extractor");
        let img = synthetic_image(10);

        let v1 = extractor.extract(&img).expect("extract first");
        let v2 = extractor.extract(&img).expect("extract second");

        assert_eq!(v1.len(), 512);
        assert!(v1.iter().all(|x| x.is_finite()));
        assert_eq!(v1, v2);
    }

    #[test]
    fn trunk_matches_candle_resnet_output() {
        let weights = temp_weights_path("trunk");
        let device = Device::Cpu;
        let bytes = std::fs::read(&weights.0).expect("read weights");
        let vb = VarBuilder::from_buffered_safetensors(bytes, DType::F32, &device).expect("vb");
        let candle_model = candle_transformers::models::resnet::resnet18_no_final_layer(vb)
            .expect("candle resnet");
        let extractor =
            Extractor::load(&test_config(weights.0.clone(), 64)).expect("load extractor");
        let image = synthetic_image(30);

        let input = preprocess(&image, 64, &device).expect("preprocess");
        let expected = candle_model
            .forward(&input)
            .expect("candle forward")
            .squeeze(0)
            .expect("squeeze")
            .to_vec1::<f32>()
            .expect("vec");
        let all = extractor.extract_all(&image).expect("extract_all");

        assert_eq!(all.global, expected);
        assert_eq!(all.grid, (8, 8));
        assert_eq!(all.patches.dims(), &[64, 384]);
    }

    #[test]
    fn different_images_give_different_vectors() {
        let weights = temp_weights_path("diff");
        let cfg = test_config(weights.0.clone(), 32);
        let extractor = Extractor::load(&cfg).expect("load extractor");

        let v1 = extractor.extract(&synthetic_image(0)).expect("extract a");
        let v2 = extractor.extract(&synthetic_image(120)).expect("extract b");

        assert_ne!(v1, v2);
    }

    #[test]
    fn rejects_invalid_config_and_missing_file() {
        let zero_size = test_config(PathBuf::from("unused.safetensors"), 0);
        assert!(Extractor::load(&zero_size).is_err());

        let missing = test_config(PathBuf::from("/nonexistent/resnet18.safetensors"), 32);
        assert!(Extractor::load(&missing).is_err());
    }
}

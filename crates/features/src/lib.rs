use std::path::PathBuf;

use candle_core::{DType, Device, Tensor};
use candle_nn::{Func, Module, VarBuilder};
use candle_transformers::models::resnet;
use image::{imageops::FilterType, DynamicImage};
use thiserror::Error;

const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];
const PIXEL_SCALE: f64 = 255.0;

// Temporary stub until the `config` crate (issue #2) is merged.
#[derive(Debug, Clone)]
pub struct Config {
    pub model_weights_path: PathBuf,
    pub input_size: usize,
}

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
    model: Func<'static>,
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
        let model = resnet::resnet18_no_final_layer(vb)?;

        Ok(Self {
            model,
            input_size: config.input_size,
            device,
        })
    }

    pub fn extract(&self, image: &DynamicImage) -> Result<Vec<f32>, FeaturesError> {
        let input = preprocess(image, self.input_size, &self.device)?;
        let features = self.model.forward(&input)?;
        Ok(features.squeeze(0)?.to_vec1::<f32>()?)
    }
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
        let _model = resnet::resnet18_no_final_layer(vb).expect("build resnet18 graph");
        varmap.save(path).expect("save test safetensors");
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
        let cfg = Config {
            model_weights_path: weights.0.clone(),
            input_size: 32,
        };
        let extractor = Extractor::load(&cfg).expect("load extractor");
        let img = synthetic_image(10);

        let v1 = extractor.extract(&img).expect("extract first");
        let v2 = extractor.extract(&img).expect("extract second");

        assert_eq!(v1.len(), 512);
        assert!(v1.iter().all(|x| x.is_finite()));
        assert_eq!(v1, v2);
    }

    #[test]
    fn different_images_give_different_vectors() {
        let weights = temp_weights_path("diff");
        let cfg = Config {
            model_weights_path: weights.0.clone(),
            input_size: 32,
        };
        let extractor = Extractor::load(&cfg).expect("load extractor");

        let v1 = extractor.extract(&synthetic_image(0)).expect("extract a");
        let v2 = extractor.extract(&synthetic_image(120)).expect("extract b");

        assert_ne!(v1, v2);
    }

    #[test]
    fn rejects_invalid_config_and_missing_file() {
        let zero_size = Config {
            model_weights_path: PathBuf::from("unused.safetensors"),
            input_size: 0,
        };
        assert!(Extractor::load(&zero_size).is_err());

        let missing = Config {
            model_weights_path: PathBuf::from("/nonexistent/resnet18.safetensors"),
            input_size: 32,
        };
        assert!(Extractor::load(&missing).is_err());
    }
}

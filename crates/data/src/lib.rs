use std::path::{Path, PathBuf};

use config::{Perturbation, PerturbationKind};
use image::DynamicImage;
use thiserror::Error;

pub type Result<T, E = DataError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum DataError {
    #[error("directory not found: {path}")]
    NotFound { path: PathBuf },
    #[error("path is not a directory: {path}")]
    NotADirectory { path: PathBuf },
    #[error("failed to read directory {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Lists image files in a directory (not recursive), sorted by path.
pub fn list_images(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        return Err(DataError::NotFound {
            path: path.to_path_buf(),
        });
    }
    if !path.is_dir() {
        return Err(DataError::NotADirectory {
            path: path.to_path_buf(),
        });
    }

    let io_err = |source| DataError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path).map_err(io_err)? {
        let entry_path = entry.map_err(io_err)?.path();
        if entry_path.is_file() && is_image_extension(&entry_path) {
            files.push(entry_path);
        }
    }
    // Sorted so results are the same on every machine.
    files.sort();
    Ok(files)
}

/// Reads all images from a directory, skipping files that are not images.
pub fn load_images(path: &Path) -> Result<Vec<(PathBuf, DynamicImage)>> {
    let mut images = Vec::new();
    for file in list_images(path)? {
        if let Ok(img) = image::open(&file) {
            images.push((file, img));
        }
    }
    Ok(images)
}

/// Returns a changed copy of `image`; `amount` is in the unit of the kind
/// (brightness and hue in image-crate steps, contrast in percent, blur as sigma).
pub fn perturb(image: &DynamicImage, change: &Perturbation) -> DynamicImage {
    match change.kind {
        PerturbationKind::Brightness => image.brighten(change.amount as i32),
        PerturbationKind::Contrast => image.adjust_contrast(change.amount),
        PerturbationKind::Blur => image.blur(change.amount),
        PerturbationKind::Hue => image.huerotate(change.amount as i32),
    }
}

/// True when the file name ends in an extension of a format the image crate can read.
pub fn is_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| image::ImageFormat::from_extension(ext).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "prometheus_x_data_test_{name}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&path).expect("create test temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write_synthetic_image(path: &Path, color: [u8; 3]) {
        let img = RgbImage::from_fn(8, 8, |_x, _y| Rgb(color));
        img.save(path).expect("save synthetic image");
    }

    #[test]
    fn loads_valid_images_and_skips_non_images() {
        let temp = TempDir::new("valid");
        let img1 = temp.path().join("a.png");
        let img2 = temp.path().join("b.png");
        let non_img = temp.path().join("notes.txt");

        write_synthetic_image(&img1, [255, 0, 0]);
        write_synthetic_image(&img2, [0, 255, 0]);
        std::fs::write(&non_img, b"not an image").expect("write non-image file");

        let loaded = load_images(temp.path()).expect("load images");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].0, img1);
        assert_eq!(loaded[1].0, img2);
    }

    #[test]
    fn list_images_filters_by_extension() {
        let temp = TempDir::new("list");
        write_synthetic_image(&temp.path().join("a.png"), [1, 2, 3]);
        std::fs::write(temp.path().join("notes.txt"), b"x").expect("write file");
        let listed = list_images(temp.path()).expect("list images");
        assert_eq!(listed, vec![temp.path().join("a.png")]);
    }

    #[test]
    fn perturb_changes_pixels_and_keeps_size() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(8, 8, |_, _| Rgb([100, 100, 100])));
        let change = Perturbation {
            name: "dark".into(),
            kind: PerturbationKind::Brightness,
            amount: -50.0,
        };
        let out = perturb(&img, &change);
        assert_eq!((out.width(), out.height()), (8, 8));
        assert!(out.to_rgb8().get_pixel(0, 0)[0] < 100);
    }

    #[test]
    fn missing_directory_returns_error() {
        let missing = Path::new("nonexistent_prometheus_x_dir_12345");
        let err = load_images(missing).expect_err("should fail on missing dir");
        assert!(matches!(err, DataError::NotFound { path } if path == missing));
    }

    #[test]
    fn file_path_returns_error() {
        let temp = TempDir::new("file_not_dir");
        let file_path = temp.path().join("single.txt");
        std::fs::write(&file_path, b"hello").expect("write file");

        let err = load_images(&file_path).expect_err("should fail when path is file");
        assert!(matches!(err, DataError::NotADirectory { path } if path == file_path));
    }

    #[test]
    fn empty_directory_returns_empty_list() {
        let temp = TempDir::new("empty");
        let loaded = load_images(temp.path()).expect("load images from empty dir");
        assert!(loaded.is_empty());
    }
}

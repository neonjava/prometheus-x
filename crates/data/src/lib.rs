use std::path::{Path, PathBuf};

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

// Reads all images from a directory, skipping non-image files.
pub fn load_images(path: &Path) -> Result<Vec<(PathBuf, DynamicImage)>> {
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

    let read_dir = std::fs::read_dir(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut images = Vec::new();
    for entry_res in read_dir {
        let entry = entry_res.map_err(|source| DataError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let entry_path = entry.path();
        // Skip subdirectories to avoid recursing into nested folders.
        if !entry_path.is_file() {
            continue;
        }

        // Attempt image decode; skip files that are not valid images.
        if let Ok(img) = image::open(&entry_path) {
            images.push((entry_path, img));
        }
    }

    // Sort by path for deterministic ordering across environments.
    images.sort_by(|(a, _), (b, _)| a.cmp(b));

    Ok(images)
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

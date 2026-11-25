use std::collections::HashMap;
use std::path::Path;

use candle_core::{safetensors, Device, Tensor, D};
use rand::rngs::StdRng;
use rand::seq::index::sample;

use crate::DetectorError;

const BANK_KEY: &str = "bank";

/// Patch features of good training images (PatchCore-style memory bank).
pub struct PatchBank {
    rows: Tensor,
    /// Squared length of every bank row, shape (1, rows); reused for each score.
    sq_norms: Tensor,
}

/// Largest patch distance of one image, plus the distance of every patch.
pub struct PatchScore {
    pub score: f32,
    pub map: Vec<f32>,
    pub grid: (usize, usize),
}

impl PatchBank {
    pub fn from_rows(rows: Tensor) -> Result<Self, DetectorError> {
        let sq_norms = rows.sqr()?.sum(D::Minus1)?.unsqueeze(0)?;
        Ok(Self { rows, sq_norms })
    }

    pub fn len(&self) -> usize {
        self.rows.dims().first().copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn save(&self, path: &Path) -> Result<(), DetectorError> {
        let tensors = HashMap::from([(BANK_KEY.to_string(), self.rows.clone())]);
        safetensors::save(&tensors, path)?;
        Ok(())
    }

    pub fn load(path: &Path, device: &Device) -> Result<Self, DetectorError> {
        let mut tensors = safetensors::load(path, device)?;
        let rows = tensors
            .remove(BANK_KEY)
            .ok_or_else(|| DetectorError::BadBank(path.to_path_buf()))?;
        Self::from_rows(rows)
    }

    /// Nearest-bank distance for each patch row; the image score is the largest one.
    pub fn score(
        &self,
        patches: &Tensor,
        grid: (usize, usize),
    ) -> Result<PatchScore, DetectorError> {
        if self.is_empty() {
            return Err(DetectorError::EmptyReference);
        }
        let patch_sq = patches.sqr()?.sum_keepdim(D::Minus1)?;
        let cross = patches.matmul(&self.rows.t()?)?;
        // |p - b|^2 = |p|^2 + |b|^2 - 2 p.b; clamp tiny negatives from rounding.
        let sq_dist = patch_sq
            .broadcast_add(&self.sq_norms)?
            .broadcast_sub(&cross.affine(2.0, 0.0)?)?
            .relu()?;
        let map = sq_dist.min(D::Minus1)?.sqrt()?.to_vec1::<f32>()?;
        let score = map.iter().copied().fold(0.0, f32::max);
        Ok(PatchScore { score, map, grid })
    }
}

/// Keeps `count` randomly chosen rows (all rows if there are fewer).
pub fn sample_rows(
    patches: &Tensor,
    count: usize,
    rng: &mut StdRng,
) -> Result<Tensor, DetectorError> {
    let total = patches.dim(0)?;
    let picked: Vec<u32> = sample(rng, total, count.min(total))
        .into_iter()
        .map(|i| i as u32)
        .collect();
    let index = Tensor::from_vec(picked.clone(), picked.len(), patches.device())?;
    Ok(patches.index_select(&index, 0)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn rows(values: &[[f32; 2]]) -> Tensor {
        let flat: Vec<f32> = values.iter().flatten().copied().collect();
        Tensor::from_vec(flat, (values.len(), 2), &Device::Cpu).expect("tensor")
    }

    #[test]
    fn score_is_largest_nearest_distance() {
        let bank = PatchBank::from_rows(rows(&[[0.0, 0.0], [10.0, 0.0]])).expect("bank");
        let image = rows(&[[1.0, 0.0], [10.0, 4.0]]);
        let result = bank.score(&image, (1, 2)).expect("score");
        assert!((result.map[0] - 1.0).abs() < 1e-5);
        assert!((result.map[1] - 4.0).abs() < 1e-5);
        assert!((result.score - 4.0).abs() < 1e-5);
    }

    #[test]
    fn bank_save_then_load_scores_the_same() {
        let bank = PatchBank::from_rows(rows(&[[0.0, 0.0], [3.0, 4.0]])).expect("bank");
        let path = std::env::temp_dir().join(format!("px_bank_{}.safetensors", std::process::id()));
        bank.save(&path).expect("save");
        let loaded = PatchBank::load(&path, &Device::Cpu).expect("load");
        std::fs::remove_file(&path).expect("cleanup");
        let image = rows(&[[3.0, 8.0]]);
        let a = bank.score(&image, (1, 1)).expect("a").score;
        let b = loaded.score(&image, (1, 1)).expect("b").score;
        assert_eq!(a, b);
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn sampling_keeps_requested_count() {
        let mut rng = StdRng::seed_from_u64(1);
        let picked =
            sample_rows(&rows(&[[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]]), 2, &mut rng).expect("sample");
        assert_eq!(picked.dims(), &[2, 2]);
    }
}

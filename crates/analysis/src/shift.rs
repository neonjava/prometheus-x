use crate::{pick_threshold, AnalysisError};

/// Where the good training images sit in feature space.
pub struct ShiftReference {
    mean: Vec<f32>,
    limit: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Shift {
    pub distance: f32,
    pub limit: f32,
    /// True when the image is farther from the training mean than almost every training image.
    pub warning: bool,
}

impl ShiftReference {
    /// `limit` is the given percentile of training distances to the training mean.
    pub fn fit(vectors: &[Vec<f32>], percentile: f32) -> Result<Self, AnalysisError> {
        let first = vectors.first().ok_or(AnalysisError::EmptyScores)?;
        let mut mean = vec![0.0; first.len()];
        for vector in vectors {
            for (m, v) in mean.iter_mut().zip(vector) {
                *m += v / vectors.len() as f32;
            }
        }
        let distances: Vec<f32> = vectors.iter().map(|v| distance(v, &mean)).collect();
        Ok(Self {
            limit: pick_threshold(&distances, percentile)?,
            mean,
        })
    }

    pub fn check(&self, vector: &[f32]) -> Shift {
        let distance = distance(vector, &self.mean);
        Shift {
            distance,
            limit: self.limit,
            warning: distance > self.limit,
        }
    }
}

fn distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_vector_warns_and_typical_one_does_not() {
        let train = vec![
            vec![0.0, 0.0],
            vec![2.0, 0.0],
            vec![0.0, 2.0],
            vec![2.0, 2.0],
        ];
        let reference = ShiftReference::fit(&train, 100.0).expect("fit");
        assert!(!reference.check(&[1.0, 1.0]).warning);
        assert!(reference.check(&[20.0, 20.0]).warning);
    }
}

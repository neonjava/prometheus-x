use crate::AnalysisError;

/// Logistic curve that turns a score into a defect probability.
/// It is centred on the threshold and scaled by the spread of good validation scores.
#[derive(Debug, Clone, Copy)]
pub struct Logistic {
    center: f32,
    scale: f32,
}

impl Logistic {
    pub fn fit(val_scores: &[f32], threshold: f32) -> Result<Self, AnalysisError> {
        if val_scores.is_empty() {
            return Err(AnalysisError::EmptyScores);
        }
        let n = val_scores.len() as f32;
        let mean = val_scores.iter().sum::<f32>() / n;
        let variance = val_scores
            .iter()
            .map(|s| (s - mean) * (s - mean))
            .sum::<f32>()
            / n;
        // A flat validation set would divide by zero; fall back to a unit scale.
        let scale = if variance > 0.0 { variance.sqrt() } else { 1.0 };
        Ok(Self {
            center: threshold,
            scale,
        })
    }

    pub fn defect_probability(&self, score: f32) -> f32 {
        1.0 / (1.0 + (-(score - self.center) / self.scale).exp())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_at_threshold_and_rising() {
        let curve = Logistic::fit(&[1.0, 2.0, 3.0], 2.0).expect("fit");
        assert!((curve.defect_probability(2.0) - 0.5).abs() < 1e-6);
        assert!(curve.defect_probability(5.0) > 0.9);
        assert!(curve.defect_probability(-1.0) < 0.1);
    }

    #[test]
    fn empty_scores_error() {
        assert!(Logistic::fit(&[], 1.0).is_err());
    }
}

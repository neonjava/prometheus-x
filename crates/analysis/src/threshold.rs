use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum AnalysisError {
    #[error("scores slice cannot be empty")]
    EmptyScores,
    #[error("percentile must be between 0.0 and 100.0, got {0}")]
    InvalidPercentile(f32),
    #[error("score cannot be NaN")]
    InvalidScore,
    #[error("scores and labels must have the same length")]
    LengthMismatch,
    #[error("need at least one good and one defect image")]
    NeedBothClasses,
}

// Compute threshold from validation scores at the specified percentile using linear interpolation.
pub fn pick_threshold(scores: &[f32], percentile: f32) -> Result<f32, AnalysisError> {
    if scores.is_empty() {
        return Err(AnalysisError::EmptyScores);
    }
    if percentile.is_nan() || !(0.0..=100.0).contains(&percentile) {
        return Err(AnalysisError::InvalidPercentile(percentile));
    }
    for &score in scores {
        if score.is_nan() {
            return Err(AnalysisError::InvalidScore);
        }
    }

    let mut sorted = scores.to_vec();
    // Sort in ascending order to map percentiles to score ranks.
    sorted.sort_by(|a, b| a.total_cmp(b));

    if sorted.len() == 1 {
        return Ok(sorted[0]);
    }

    // Map percentile to rank position in [0, sorted.len() - 1].
    let rank = (percentile / 100.0) * (sorted.len() - 1) as f32;
    let lower_idx = (rank.floor() as usize).min(sorted.len() - 1);
    let upper_idx = (rank.ceil() as usize).min(sorted.len() - 1);

    if lower_idx == upper_idx {
        Ok(sorted[lower_idx])
    } else {
        // Interpolate linearly between adjacent ranks.
        let weight = rank - lower_idx as f32;
        Ok(sorted[lower_idx] + weight * (sorted[upper_idx] - sorted[lower_idx]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_scores_percentiles() {
        let scores = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(pick_threshold(&scores, 0.0).unwrap(), 1.0);
        assert_eq!(pick_threshold(&scores, 25.0).unwrap(), 2.0);
        assert_eq!(pick_threshold(&scores, 50.0).unwrap(), 3.0);
        assert_eq!(pick_threshold(&scores, 75.0).unwrap(), 4.0);
        assert_eq!(pick_threshold(&scores, 100.0).unwrap(), 5.0);
    }

    #[test]
    fn test_interpolation_even_elements() {
        let scores = [10.0, 20.0, 30.0, 40.0];
        // 90th percentile: rank = 0.9 * 3 = 2.7 -> 30.0 + 0.7 * 10.0 = 37.0
        let threshold = pick_threshold(&scores, 90.0).unwrap();
        assert!((threshold - 37.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_single_score() {
        let scores = [42.0];
        assert_eq!(pick_threshold(&scores, 0.0).unwrap(), 42.0);
        assert_eq!(pick_threshold(&scores, 50.0).unwrap(), 42.0);
        assert_eq!(pick_threshold(&scores, 100.0).unwrap(), 42.0);
    }

    #[test]
    fn test_unsorted_scores() {
        let unsorted = [5.0, 1.0, 4.0, 2.0, 3.0];
        assert_eq!(pick_threshold(&unsorted, 50.0).unwrap(), 3.0);
    }

    #[test]
    fn test_empty_input_errors() {
        let empty: [f32; 0] = [];
        let err = pick_threshold(&empty, 95.0).unwrap_err();
        assert_eq!(err, AnalysisError::EmptyScores);
    }

    #[test]
    fn test_invalid_percentile_errors() {
        let scores = [1.0, 2.0, 3.0];
        assert_eq!(
            pick_threshold(&scores, -1.0).unwrap_err(),
            AnalysisError::InvalidPercentile(-1.0)
        );
        assert_eq!(
            pick_threshold(&scores, 101.0).unwrap_err(),
            AnalysisError::InvalidPercentile(101.0)
        );
        let nan_err = pick_threshold(&scores, f32::NAN).unwrap_err();
        assert!(matches!(nan_err, AnalysisError::InvalidPercentile(_)));
    }

    #[test]
    fn test_nan_score_errors() {
        let scores = [1.0, f32::NAN, 3.0];
        assert_eq!(
            pick_threshold(&scores, 50.0).unwrap_err(),
            AnalysisError::InvalidScore
        );
    }
}

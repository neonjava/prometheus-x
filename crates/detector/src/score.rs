use crate::DetectorError;

/// Distance from `vector` to its nearest good vector. Higher means more unusual.
pub fn global_score(vector: &[f32], good_vectors: &[Vec<f32>]) -> Result<f32, DetectorError> {
    let mut best = f32::INFINITY;
    for good in good_vectors {
        if good.len() != vector.len() {
            return Err(DetectorError::DimensionMismatch {
                expected: good.len(),
                got: vector.len(),
            });
        }
        best = best.min(squared_distance(vector, good));
    }
    if good_vectors.is_empty() {
        return Err(DetectorError::EmptyReference);
    }
    Ok(best.sqrt())
}

fn squared_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearer_vector_scores_lower() {
        let good = vec![vec![0.0, 0.0], vec![10.0, 10.0]];
        let near = global_score(&[1.0, 0.0], &good).expect("near score");
        let far = global_score(&[5.0, 5.0], &good).expect("far score");
        assert!((near - 1.0).abs() < 1e-6);
        assert!(far > near);
    }

    #[test]
    fn empty_or_mismatched_reference_errors() {
        assert!(matches!(
            global_score(&[1.0], &[]),
            Err(DetectorError::EmptyReference)
        ));
        assert!(matches!(
            global_score(&[1.0], &[vec![1.0, 2.0]]),
            Err(DetectorError::DimensionMismatch { .. })
        ));
    }
}

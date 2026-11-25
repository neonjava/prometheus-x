use crate::AnalysisError;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub auroc: f32,
    /// Share of good images flagged as defect.
    pub fpr: f32,
    /// Share of defect images missed (called good).
    pub miss_rate: f32,
}

/// `labels[i]` is true when image i is a real defect; a score above `threshold` is flagged.
pub fn evaluate(scores: &[f32], labels: &[bool], threshold: f32) -> Result<Metrics, AnalysisError> {
    if scores.len() != labels.len() {
        return Err(AnalysisError::LengthMismatch);
    }
    let defects = labels.iter().filter(|&&l| l).count();
    let goods = labels.len() - defects;
    if defects == 0 || goods == 0 {
        return Err(AnalysisError::NeedBothClasses);
    }

    let flagged = |i: usize| scores[i] > threshold;
    let false_pos = (0..scores.len())
        .filter(|&i| !labels[i] && flagged(i))
        .count();
    let missed = (0..scores.len())
        .filter(|&i| labels[i] && !flagged(i))
        .count();

    Ok(Metrics {
        auroc: auroc(scores, labels, defects, goods),
        fpr: false_pos as f32 / goods as f32,
        miss_rate: missed as f32 / defects as f32,
    })
}

// Chance a random defect scores above a random good one; ties count half.
fn auroc(scores: &[f32], labels: &[bool], defects: usize, goods: usize) -> f32 {
    let mut wins = 0.0_f64;
    for (i, _) in labels.iter().enumerate().filter(|(_, &l)| l) {
        for (j, _) in labels.iter().enumerate().filter(|(_, &l)| !l) {
            if scores[i] > scores[j] {
                wins += 1.0;
            } else if scores[i] == scores[j] {
                wins += 0.5;
            }
        }
    }
    (wins / (defects * goods) as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_separation() {
        let m = evaluate(&[0.1, 0.2, 0.8, 0.9], &[false, false, true, true], 0.5).expect("metrics");
        assert_eq!((m.auroc, m.fpr, m.miss_rate), (1.0, 0.0, 0.0));
    }

    #[test]
    fn mixed_case_matches_hand_computation() {
        // goods: 0.2, 0.6   defects: 0.4, 0.9   threshold 0.5
        let m = evaluate(&[0.2, 0.6, 0.4, 0.9], &[false, false, true, true], 0.5).expect("metrics");
        // pairs (defect>good): 0.4>0.2, 0.9>0.2, 0.9>0.6 => 3 of 4
        assert!((m.auroc - 0.75).abs() < 1e-6);
        assert!((m.fpr - 0.5).abs() < 1e-6);
        assert!((m.miss_rate - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(
            evaluate(&[0.1], &[true, false], 0.5),
            Err(AnalysisError::LengthMismatch)
        );
        assert_eq!(
            evaluate(&[0.1, 0.2], &[true, true], 0.5),
            Err(AnalysisError::NeedBothClasses)
        );
    }
}

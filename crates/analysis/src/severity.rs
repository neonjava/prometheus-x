use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Low,
    Medium,
    High,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Share of patches (in percent) whose distance is above `threshold`.
pub fn area_percent(map: &[f32], threshold: f32) -> f32 {
    if map.is_empty() {
        return 0.0;
    }
    let over = map.iter().filter(|&&d| d > threshold).count();
    100.0 * over as f32 / map.len() as f32
}

/// Index = defect area % x score, with the score taken relative to its threshold.
pub fn severity(area_pct: f32, score: f32, threshold: f32, low_max: f32, med_max: f32) -> Severity {
    let index = area_pct * score / threshold;
    if index <= low_max {
        Severity::Low
    } else if index <= med_max {
        Severity::Medium
    } else {
        Severity::High
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_counts_patches_over_threshold() {
        assert_eq!(area_percent(&[1.0, 2.0, 3.0, 4.0], 2.5), 50.0);
        assert_eq!(area_percent(&[], 1.0), 0.0);
    }

    #[test]
    fn buckets_follow_the_index() {
        assert_eq!(severity(2.0, 1.0, 1.0, 5.0, 15.0), Severity::Low);
        assert_eq!(severity(10.0, 1.0, 1.0, 5.0, 15.0), Severity::Medium);
        assert_eq!(severity(10.0, 2.0, 1.0, 5.0, 15.0), Severity::High);
    }
}

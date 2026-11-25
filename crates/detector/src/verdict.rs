use std::fmt;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Verdict {
    Good,
    Defect,
    Uncertain,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// `margin` is relative: Uncertain covers threshold * (1 +- margin).
pub fn verdict(score: f32, threshold: f32, margin: f32) -> Verdict {
    let band = threshold * margin;
    if score > threshold + band {
        Verdict::Defect
    } else if score >= threshold - band {
        Verdict::Uncertain
    } else {
        Verdict::Good
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_all_three_zones_and_boundaries() {
        assert_eq!(verdict(0.5, 1.0, 0.1), Verdict::Good);
        assert_eq!(verdict(0.9, 1.0, 0.1), Verdict::Uncertain);
        assert_eq!(verdict(1.0, 1.0, 0.1), Verdict::Uncertain);
        assert_eq!(verdict(1.1, 1.0, 0.1), Verdict::Uncertain);
        assert_eq!(verdict(1.2, 1.0, 0.1), Verdict::Defect);
    }
}

use std::path::Path;
use std::time::Instant;

use analysis::{area_percent, severity, Logistic, Severity, Shift, ShiftReference};
use config::Config;
use features::Extractor;
use image::DynamicImage;

use crate::patch::{PatchBank, PatchScore};
use crate::store::{open_image, Reference};
use crate::{fuse, global_score, verdict, DetectorError, Verdict};

/// One model's opinion on an image.
#[derive(Debug, Clone, Copy)]
pub struct ModelResult {
    pub score: f32,
    pub threshold: f32,
    pub vote: Verdict,
}

/// One line of the step-by-step trace, with the time since the previous line.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub message: String,
    pub ms: u64,
}

/// Where and how bad; only exists when the patch model ran.
#[derive(Debug, Clone)]
pub struct Finding {
    /// Distance of every patch to the good bank, row by row over `grid`.
    pub map: Vec<f32>,
    pub grid: (usize, usize),
    pub area_pct: f32,
    pub severity: Severity,
    pub defect_probability: f32,
}

#[derive(Debug, Clone)]
pub struct Prediction {
    pub verdict: Verdict,
    /// True when a human should look: the models disagree or a score is near its threshold.
    pub needs_review: bool,
    pub global: ModelResult,
    /// None when Step 1 passed the image and Step 2 was skipped.
    pub patch: Option<ModelResult>,
    pub finding: Option<Finding>,
    pub shift: Shift,
    pub log: Vec<LogLine>,
}

/// Loaded models: kept in memory between predictions.
pub struct Predictor {
    extractor: Extractor,
    reference: Reference,
    bank: PatchBank,
    shift: ShiftReference,
    patch_curve: Logistic,
    margin: f32,
    severity_low_max: f32,
    severity_med_max: f32,
}

struct Trace {
    lines: Vec<LogLine>,
    last: Instant,
}

impl Trace {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            last: Instant::now(),
        }
    }

    fn add(&mut self, message: String) {
        let now = Instant::now();
        let ms = now.duration_since(self.last).as_millis() as u64;
        self.last = now;
        self.lines.push(LogLine { message, ms });
    }
}

impl Predictor {
    pub fn load(cfg: &Config) -> Result<Self, DetectorError> {
        let extractor = Extractor::load(cfg)?;
        let bank = PatchBank::load(&cfg.patch_bank_path, extractor.device())?;
        let reference = Reference::load(&cfg.good_vectors_path)?;
        Ok(Self {
            shift: ShiftReference::fit(&reference.vectors, cfg.shift_percentile)?,
            patch_curve: Logistic::fit(&reference.patch_val_scores, reference.patch_threshold)?,
            extractor,
            bank,
            reference,
            margin: cfg.uncertain_margin,
            severity_low_max: cfg.severity_low_max,
            severity_med_max: cfg.severity_med_max,
        })
    }

    /// Step 1 quick global check; only odd images go on to Step 2 and possibly Step 3.
    pub fn predict(&self, image: &DynamicImage) -> Result<Prediction, DetectorError> {
        self.analyze(image, false)
    }

    /// Like `predict`, but `always_step2` also scores images that pass Step 1.
    /// Evaluation needs both models' scores on every image.
    pub fn analyze(
        &self,
        image: &DynamicImage,
        always_step2: bool,
    ) -> Result<Prediction, DetectorError> {
        let mut trace = Trace::new();
        let features = self.extractor.extract_all(image)?;
        trace.add(format!(
            "Features extracted: {}-dim vector, {}x{} patches",
            features.global.len(),
            features.grid.0,
            features.grid.1
        ));
        let shift = self.shift.check(&features.global);
        trace.add(shift_message(&shift));

        let global = self.vote(
            global_score(&features.global, &self.reference.vectors)?,
            self.reference.threshold,
        );
        let passed_step1 = global.vote == Verdict::Good;
        if passed_step1 && !always_step2 {
            trace.add(format!(
                "Step 1 quick check: score {:.2} vs threshold {:.2} -> looks normal. PASS, Step 2 skipped",
                global.score, global.threshold
            ));
            return Ok(Prediction {
                verdict: Verdict::Good,
                needs_review: false,
                global,
                patch: None,
                finding: None,
                shift,
                log: trace.lines,
            });
        }
        trace.add(format!(
            "Step 1 quick check: score {:.2} vs threshold {:.2} -> {}. Running Step 2",
            global.score,
            global.threshold,
            if passed_step1 {
                "looks normal (forced)"
            } else {
                "looks odd"
            }
        ));

        let patch_score = self.bank.score(&features.patches, features.grid)?;
        let patch = self.vote(patch_score.score, self.reference.patch_threshold);
        trace.add(format!(
            "Step 2 closer look: patch score {:.2} vs threshold {:.2} -> {}",
            patch.score, patch.threshold, patch.vote
        ));

        let verdict = if passed_step1 {
            Verdict::Good
        } else {
            fuse(global.vote, patch.vote)
        };
        let finding = self.finding(patch_score, &patch);
        trace.add(decision_message(verdict, &finding));
        Ok(Prediction {
            verdict,
            needs_review: verdict == Verdict::Uncertain,
            global,
            patch: Some(patch),
            finding: Some(finding),
            shift,
            log: trace.lines,
        })
    }

    fn vote(&self, score: f32, threshold: f32) -> ModelResult {
        ModelResult {
            score,
            threshold,
            vote: verdict(score, threshold, self.margin),
        }
    }

    fn finding(&self, patch_score: PatchScore, patch: &ModelResult) -> Finding {
        let area_pct = area_percent(&patch_score.map, patch.threshold);
        Finding {
            area_pct,
            severity: severity(
                area_pct,
                patch.score,
                patch.threshold,
                self.severity_low_max,
                self.severity_med_max,
            ),
            defect_probability: self.patch_curve.defect_probability(patch.score),
            map: patch_score.map,
            grid: patch_score.grid,
        }
    }
}

fn shift_message(shift: &Shift) -> String {
    if shift.warning {
        format!(
            "Distribution check: distance {:.2} > limit {:.2} -> WARNING, lighting or camera may have changed",
            shift.distance, shift.limit
        )
    } else {
        format!(
            "Distribution check: distance {:.2} <= limit {:.2} -> OK, looks like the training images",
            shift.distance, shift.limit
        )
    }
}

fn decision_message(verdict: Verdict, finding: &Finding) -> String {
    match verdict {
        Verdict::Defect => format!(
            "Do both agree? Yes, both flag damage -> REJECTED. {:.1}% of patches affected, severity {}, defect probability {:.0}%",
            finding.area_pct,
            finding.severity,
            100.0 * finding.defect_probability
        ),
        Verdict::Good => "Step 1 passed the image -> OK".to_string(),
        Verdict::Uncertain => "Do both agree? No -> Step 3: sent to a person for review".to_string(),
    }
}

/// One-shot prediction from a file path. Loads the models every call.
pub fn predict(cfg: &Config, image_path: &Path) -> Result<Prediction, DetectorError> {
    let image = open_image(image_path)?;
    Predictor::load(cfg)?.predict(&image)
}

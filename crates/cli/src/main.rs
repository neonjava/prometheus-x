use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use config::Perturbation;
use detector::store::{build_reference, open_image};
use detector::{ModelResult, Predictor, Verdict};
use features::Extractor;
use image::DynamicImage;

#[derive(Parser)]
#[command(
    name = "prometheus-x",
    about = "Open-set anomaly detection trained on good images only"
)]
struct Cli {
    /// Path to the config file.
    #[arg(long, default_value = "config.toml", global = true)]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the good-vector reference from train_dir and calibrate the threshold on val_dir.
    Build,
    /// Predict one image.
    Predict { image: PathBuf },
    /// Score test_dir/good and test_dir/defect and print metrics.
    Evaluate,
    /// Re-test on darker, brighter, blurry and colour-shifted copies of test images.
    Robustness,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;
    match cli.command {
        Command::Build => build(&cfg),
        Command::Predict { image } => predict(&cfg, &image),
        Command::Evaluate => evaluate(&cfg),
        Command::Robustness => robustness(&cfg),
    }
}

fn build(cfg: &Config) -> Result<()> {
    let started = Instant::now();
    let extractor = Extractor::load(cfg)?;
    let (reference, bank) = build_reference(cfg, &extractor)?;
    reference.save(&cfg.good_vectors_path)?;
    bank.save(&cfg.patch_bank_path)?;
    println!(
        "global: {} vectors, threshold {:.4} | patch: {} rows, threshold {:.4} | p{} | {:.1}s",
        reference.vectors.len(),
        reference.threshold,
        bank.len(),
        reference.patch_threshold,
        cfg.threshold_percentile,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

fn print_model(name: &str, model: &ModelResult) {
    println!(
        "  {name:<7} score={:.4} threshold={:.4} vote={}",
        model.score, model.threshold, model.vote
    );
}

fn predict(cfg: &Config, image_path: &Path) -> Result<()> {
    let predictor = Predictor::load(cfg)?;
    let prediction = predictor.predict(&open_image(image_path)?)?;
    for line in &prediction.log {
        println!("[{:>4} ms] {}", line.ms, line.message);
    }
    println!(
        "verdict={} needs_review={}",
        prediction.verdict, prediction.needs_review
    );
    print_model("global", &prediction.global);
    if let Some(patch) = &prediction.patch {
        print_model("patch", patch);
    }
    Ok(())
}

/// A prediction where both models ran.
struct Full {
    shift_warning: bool,
    verdict: Verdict,
    global: ModelResult,
    patch: ModelResult,
}

fn analyze_image(predictor: &Predictor, image: &DynamicImage) -> Result<Full> {
    let p = predictor.analyze(image, true)?;
    let patch = p.patch.context("patch model did not run")?;
    Ok(Full {
        shift_warning: p.shift.warning,
        verdict: p.verdict,
        global: p.global,
        patch,
    })
}

fn analyze_dir(predictor: &Predictor, dir: &Path) -> Result<Vec<Full>> {
    data::list_images(dir)?
        .iter()
        .map(|path| analyze_image(predictor, &open_image(path)?))
        .collect()
}

fn print_metrics(
    name: &str,
    good: &[Full],
    defect: &[Full],
    pick: impl Fn(&Full) -> ModelResult,
) -> Result<()> {
    let results: Vec<ModelResult> = good.iter().chain(defect).map(&pick).collect();
    let scores: Vec<f32> = results.iter().map(|r| r.score).collect();
    let labels: Vec<bool> = good
        .iter()
        .map(|_| false)
        .chain(defect.iter().map(|_| true))
        .collect();
    let metrics = analysis::evaluate(&scores, &labels, results[0].threshold)?;
    println!(
        "{name:<7} AUROC {:.4} | false-positive rate {:.4} | miss rate {:.4}",
        metrics.auroc, metrics.fpr, metrics.miss_rate
    );
    Ok(())
}

fn print_zones(name: &str, results: &[Full]) {
    let count = |wanted| results.iter().filter(|r| r.verdict == wanted).count();
    println!(
        "{name}: Good {} / Uncertain {} / Defect {}",
        count(Verdict::Good),
        count(Verdict::Uncertain),
        count(Verdict::Defect)
    );
}

fn evaluate(cfg: &Config) -> Result<()> {
    let predictor = Predictor::load(cfg)?;
    let good = analyze_dir(&predictor, &cfg.test_dir.join("good")).context("test/good")?;
    let defect = analyze_dir(&predictor, &cfg.test_dir.join("defect")).context("test/defect")?;

    println!("images: {} good, {} defect", good.len(), defect.len());
    print_metrics("global", &good, &defect, |r| r.global)?;
    print_metrics("patch", &good, &defect, |r| r.patch)?;
    println!("final verdicts (Step 1 pass, else Step 2 fusion):");
    print_zones("  good  ", &good);
    print_zones("  defect", &defect);
    Ok(())
}

/// Evenly spaced images from a folder, so the sample covers the whole set.
fn load_sample(dir: &Path, count: usize) -> Result<Vec<DynamicImage>> {
    let paths = data::list_images(dir)?;
    let step = paths.len().div_ceil(count.max(1)).max(1);
    paths
        .iter()
        .step_by(step)
        .take(count)
        .map(|path| open_image(path).map_err(Into::into))
        .collect()
}

fn analyze_changed(
    predictor: &Predictor,
    images: &[DynamicImage],
    change: Option<&Perturbation>,
) -> Result<Vec<Full>> {
    images
        .iter()
        .map(|image| match change {
            Some(change) => analyze_image(predictor, &data::perturb(image, change)),
            None => analyze_image(predictor, image),
        })
        .collect()
}

fn percent(part: usize, total: usize) -> f32 {
    100.0 * part as f32 / total as f32
}

fn zone_counts(results: &[Full]) -> String {
    let count = |wanted| results.iter().filter(|r| r.verdict == wanted).count();
    format!(
        "{:>3}/{:>3}/{:>3}",
        count(Verdict::Good),
        count(Verdict::Uncertain),
        count(Verdict::Defect)
    )
}

fn auroc_of(good: &[Full], defect: &[Full], pick: impl Fn(&Full) -> ModelResult) -> Result<f32> {
    let results: Vec<ModelResult> = good.iter().chain(defect).map(&pick).collect();
    let scores: Vec<f32> = results.iter().map(|r| r.score).collect();
    let labels: Vec<bool> = good
        .iter()
        .map(|_| false)
        .chain(defect.iter().map(|_| true))
        .collect();
    Ok(analysis::evaluate(&scores, &labels, results[0].threshold)?.auroc)
}

fn report_condition(
    name: &str,
    predictor: &Predictor,
    good: &[DynamicImage],
    defect: &[DynamicImage],
    change: Option<&Perturbation>,
) -> Result<()> {
    let good = analyze_changed(predictor, good, change)?;
    let defect = analyze_changed(predictor, defect, change)?;
    let warned = good.iter().filter(|r| r.shift_warning).count();
    println!(
        "{name:<13} | {:>5.0}% | {} | {} | {:.3} | {:.3}",
        percent(warned, good.len()),
        zone_counts(&good),
        zone_counts(&defect),
        auroc_of(&good, &defect, |r| r.global)?,
        auroc_of(&good, &defect, |r| r.patch)?,
    );
    Ok(())
}

fn robustness(cfg: &Config) -> Result<()> {
    let predictor = Predictor::load(cfg)?;
    let good = load_sample(&cfg.test_dir.join("good"), cfg.robustness_sample)?;
    let defect = load_sample(&cfg.test_dir.join("defect"), cfg.robustness_sample)?;
    println!(
        "{} good and {} defect test images per condition",
        good.len(),
        defect.len()
    );
    println!("condition     | shift warning on good | good G/U/D | defect G/U/D | AUROC global | AUROC patch");
    report_condition("original", &predictor, &good, &defect, None)?;
    for change in &cfg.perturbations {
        report_condition(&change.name, &predictor, &good, &defect, Some(change))?;
    }
    Ok(())
}

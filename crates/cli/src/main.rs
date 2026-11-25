use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use detector::store::{build_reference, open_image};
use detector::{ModelResult, Predictor, Verdict};
use features::Extractor;

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;
    match cli.command {
        Command::Build => build(&cfg),
        Command::Predict { image } => predict(&cfg, &image),
        Command::Evaluate => evaluate(&cfg),
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
    verdict: Verdict,
    global: ModelResult,
    patch: ModelResult,
}

fn analyze_dir(predictor: &Predictor, dir: &Path) -> Result<Vec<Full>> {
    data::list_images(dir)?
        .iter()
        .map(|path| {
            let p = predictor.analyze(&open_image(path)?, true)?;
            let patch = p.patch.context("patch model did not run")?;
            Ok(Full {
                verdict: p.verdict,
                global: p.global,
                patch,
            })
        })
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

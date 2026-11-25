use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use detector::store::{build_reference, open_image};
use detector::Predictor;
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
    let reference = build_reference(cfg, &extractor)?;
    reference.save(&cfg.good_vectors_path)?;
    println!(
        "built reference: {} good vectors, threshold {:.4} (p{}), {:.1}s",
        reference.vectors.len(),
        reference.threshold,
        cfg.threshold_percentile,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

fn predict(cfg: &Config, image_path: &std::path::Path) -> Result<()> {
    let predictor = Predictor::load(cfg)?;
    let image = open_image(image_path)?;
    let prediction = predictor.predict(&image)?;
    println!(
        "verdict={} score={:.4} threshold={:.4}",
        prediction.verdict,
        prediction.score,
        predictor.threshold()
    );
    Ok(())
}

fn score_dir(predictor: &Predictor, dir: &std::path::Path) -> Result<Vec<f32>> {
    data::list_images(dir)?
        .iter()
        .map(|path| {
            let image = open_image(path)?;
            predictor.score(&image).map_err(Into::into)
        })
        .collect()
}

fn evaluate(cfg: &Config) -> Result<()> {
    let predictor = Predictor::load(cfg)?;
    let good = score_dir(&predictor, &cfg.test_dir.join("good")).context("scoring test/good")?;
    let defect =
        score_dir(&predictor, &cfg.test_dir.join("defect")).context("scoring test/defect")?;

    let scores: Vec<f32> = good.iter().chain(&defect).copied().collect();
    let labels: Vec<bool> = good
        .iter()
        .map(|_| false)
        .chain(defect.iter().map(|_| true))
        .collect();
    let metrics = analysis::evaluate(&scores, &labels, predictor.threshold())?;

    println!("images: {} good, {} defect", good.len(), defect.len());
    println!("threshold: {:.4}", predictor.threshold());
    println!("image AUROC:       {:.4}", metrics.auroc);
    println!("false-positive rate: {:.4}", metrics.fpr);
    println!("miss rate:           {:.4}", metrics.miss_rate);
    print_zones("good  ", &good, predictor.threshold(), cfg.uncertain_margin);
    print_zones(
        "defect",
        &defect,
        predictor.threshold(),
        cfg.uncertain_margin,
    );
    Ok(())
}

fn print_zones(name: &str, scores: &[f32], threshold: f32, margin: f32) {
    let count = |wanted| {
        scores
            .iter()
            .filter(|&&s| detector::verdict(s, threshold, margin) == wanted)
            .count()
    };
    println!(
        "{name}: Good {} / Uncertain {} / Defect {}",
        count(detector::Verdict::Good),
        count(detector::Verdict::Uncertain),
        count(detector::Verdict::Defect)
    );
}

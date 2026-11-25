use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use config::Config;
use detector::{LogLine, Prediction, Predictor};
use image::DynamicImage;
use serde::{Deserialize, Serialize};

const INDEX_HTML: &str = include_str!("../../../web/index.html");
const DECISIONS: [&str; 3] = ["ok", "reject", "fix"];

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct ModelResponse {
    score: f32,
    threshold: f32,
    vote: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct FindingResponse {
    grid: [usize; 2],
    map: Vec<f32>,
    area_pct: f32,
    severity: String,
    defect_probability: f32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct ShiftResponse {
    distance: f32,
    limit: f32,
    warning: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct LogResponse {
    message: String,
    ms: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct PredictionResponse {
    verdict: String,
    needs_review: bool,
    global: ModelResponse,
    patch: Option<ModelResponse>,
    finding: Option<FindingResponse>,
    shift: ShiftResponse,
    log: Vec<LogResponse>,
}

impl From<detector::ModelResult> for ModelResponse {
    fn from(m: detector::ModelResult) -> Self {
        Self {
            score: m.score,
            threshold: m.threshold,
            vote: m.vote.to_string(),
        }
    }
}

impl From<LogLine> for LogResponse {
    fn from(line: LogLine) -> Self {
        Self {
            message: line.message,
            ms: line.ms,
        }
    }
}

impl From<Prediction> for PredictionResponse {
    fn from(p: Prediction) -> Self {
        Self {
            verdict: p.verdict.to_string(),
            needs_review: p.needs_review,
            global: p.global.into(),
            patch: p.patch.map(Into::into),
            finding: p.finding.map(|f| FindingResponse {
                grid: [f.grid.0, f.grid.1],
                map: f.map,
                area_pct: f.area_pct,
                severity: f.severity.to_string(),
                defect_probability: f.defect_probability,
            }),
            shift: ShiftResponse {
                distance: p.shift.distance,
                limit: p.shift.limit,
                warning: p.shift.warning,
            },
            log: p.log.into_iter().map(Into::into).collect(),
        }
    }
}

/// Anything that can judge an image; lets tests run without model weights.
trait Model: Send + Sync + 'static {
    fn predict(&self, image: &DynamicImage) -> Result<Prediction, String>;
}

impl Model for Predictor {
    fn predict(&self, image: &DynamicImage) -> Result<Prediction, String> {
        Predictor::predict(self, image).map_err(|e| e.to_string())
    }
}

struct AppState {
    model: Box<dyn Model>,
    decisions_path: PathBuf,
}

type Shared = Arc<AppState>;
type ApiError = (StatusCode, String);

fn bad_request(message: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, message.to_string())
}

fn internal(message: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, message.to_string())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn read_image_field(mut multipart: Multipart) -> Result<(String, Vec<u8>), ApiError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(format!("invalid multipart: {e}")))?
    {
        if field.name() == Some("image") {
            let name = field.file_name().unwrap_or("upload").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| bad_request(format!("failed to read image: {e}")))?;
            return Ok((name, bytes.to_vec()));
        }
    }
    Err(bad_request("missing image field in multipart upload"))
}

async fn predict(
    State(state): State<Shared>,
    multipart: Multipart,
) -> Result<Json<PredictionResponse>, ApiError> {
    let started = Instant::now();
    let (name, bytes) = read_image_field(multipart).await?;
    let mut log = vec![LogResponse {
        message: format!("Upload received: {name} ({} KB)", bytes.len() / 1024),
        ms: started.elapsed().as_millis() as u64,
    }];

    let decode_started = Instant::now();
    let image = image::load_from_memory(&bytes)
        .map_err(|e| bad_request(format!("invalid image payload: {e}")))?;
    log.push(LogResponse {
        message: format!("Image decoded: {}x{} px", image.width(), image.height()),
        ms: decode_started.elapsed().as_millis() as u64,
    });

    // Inference is CPU-heavy, keep it off the async threads.
    let prediction = tokio::task::spawn_blocking(move || state.model.predict(&image))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    let mut response = PredictionResponse::from(prediction);
    log.append(&mut response.log);
    response.log = log;
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct DecisionRequest {
    image: String,
    verdict: String,
    decision: String,
}

#[derive(Serialize)]
struct DecisionRecord<'a> {
    time_unix: u64,
    image: &'a str,
    verdict: &'a str,
    decision: &'a str,
}

fn append_decision(path: &Path, record: &DecisionRecord) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let line = serde_json::to_string(record).map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())
}

// Step 3: a person's OK / Reject / Fix is saved so it can be audited later.
async fn decision(
    State(state): State<Shared>,
    Json(request): Json<DecisionRequest>,
) -> Result<StatusCode, ApiError> {
    if !DECISIONS.contains(&request.decision.as_str()) {
        return Err(bad_request(format!(
            "decision must be one of {DECISIONS:?}, got '{}'",
            request.decision
        )));
    }
    let time_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(internal)?
        .as_secs();
    let record = DecisionRecord {
        time_unix,
        image: &request.image,
        verdict: &request.verdict,
        decision: &request.decision,
    };
    append_decision(&state.decisions_path, &record).map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

// Axum's default 2 MB body limit would reject real PCB photos.
fn app(state: Shared, max_upload_bytes: usize) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/predict", post(predict))
        .route("/decision", post(decision))
        .layer(DefaultBodyLimit::max(max_upload_bytes))
        .with_state(state)
}

fn config_path() -> PathBuf {
    // Allow overriding default config path via command line argument.
    match std::env::args().nth(1) {
        Some(arg) => PathBuf::from(arg),
        None => PathBuf::from("config.toml"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::load(&config_path())?;
    let state: Shared = Arc::new(AppState {
        model: Box::new(Predictor::load(&cfg)?),
        decisions_path: cfg.decisions_path.clone(),
    });
    let listener = tokio::net::TcpListener::bind(&cfg.server_bind)
        .await
        .map_err(|err| anyhow::anyhow!("failed to bind {}: {err}", cfg.server_bind))?;
    println!("server listening on {}", cfg.server_bind);
    axum::serve(listener, app(state, cfg.max_upload_bytes)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use analysis::{Severity, Shift};
    use detector::{Finding, ModelResult, Verdict};
    use image::{ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const TEST_UPLOAD_LIMIT: usize = 1 << 20;
    const BOUNDARY: &str = "------------------------boundary123";

    struct StubModel;

    impl Model for StubModel {
        fn predict(&self, _image: &DynamicImage) -> Result<Prediction, String> {
            let model = ModelResult {
                score: 7.0,
                threshold: 5.0,
                vote: Verdict::Defect,
            };
            Ok(Prediction {
                verdict: Verdict::Defect,
                needs_review: false,
                global: model,
                patch: Some(model),
                finding: Some(Finding {
                    map: vec![1.0, 9.0],
                    grid: (1, 2),
                    area_pct: 50.0,
                    severity: Severity::Medium,
                    defect_probability: 0.9,
                }),
                shift: Shift {
                    distance: 1.0,
                    limit: 2.0,
                    warning: false,
                },
                log: vec![LogLine {
                    message: "stub step".to_string(),
                    ms: 1,
                }],
            })
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("px_server_{name}_{}.jsonl", std::process::id()))
    }

    async fn start(decisions_path: PathBuf) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let state = Arc::new(AppState {
            model: Box::new(StubModel),
            decisions_path,
        });
        tokio::spawn(async move {
            let _ = axum::serve(listener, app(state, TEST_UPLOAD_LIMIT)).await;
        });
        addr
    }

    async fn post_raw(
        addr: std::net::SocketAddr,
        path: &str,
        content_type: &str,
        body: &[u8],
    ) -> String {
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let mut stream = TcpStream::connect(addr).await.expect("connect to server");
        stream.write_all(head.as_bytes()).await.expect("send head");
        stream.write_all(body).await.expect("send body");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("read response");
        response
    }

    fn multipart(field: &str, filename: &str, payload: &[u8]) -> Vec<u8> {
        let mut body = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(payload);
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
        body
    }

    fn png_bytes() -> Vec<u8> {
        let img = RgbImage::from_fn(16, 16, |x, y| Rgb([x as u8, y as u8, 128]));
        let mut buffer = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
            .expect("encode synthetic png");
        buffer
    }

    #[tokio::test]
    async fn predict_returns_staged_result_with_log() {
        let addr = start(temp_path("a")).await;
        let body = multipart("image", "test.png", &png_bytes());
        let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
        let response = post_raw(addr, "/predict", &content_type, &body).await;

        assert!(response.contains("200 OK"));
        assert!(response.contains("\"verdict\":\"Defect\""));
        assert!(response.contains("\"severity\":\"Medium\""));
        assert!(response.contains("Upload received: test.png"));
        assert!(response.contains("Image decoded: 16x16 px"));
        assert!(response.contains("stub step"));
    }

    #[tokio::test]
    async fn predict_rejects_missing_image_field() {
        let addr = start(temp_path("b")).await;
        let body = multipart("other", "x.txt", b"hello");
        let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
        let response = post_raw(addr, "/predict", &content_type, &body).await;
        assert!(response.contains("400 Bad Request"));
    }

    #[tokio::test]
    async fn decision_is_saved_and_bad_values_are_rejected() {
        let path = temp_path("c");
        let addr = start(path.clone()).await;
        let good = br#"{"image":"a.jpg","verdict":"Uncertain","decision":"fix"}"#;
        let response = post_raw(addr, "/decision", "application/json", good).await;
        assert!(response.contains("204 No Content"));

        let bad = br#"{"image":"a.jpg","verdict":"Uncertain","decision":"maybe"}"#;
        let response = post_raw(addr, "/decision", "application/json", bad).await;
        assert!(response.contains("400 Bad Request"));

        let saved = std::fs::read_to_string(&path).expect("decisions file");
        std::fs::remove_file(&path).expect("cleanup");
        assert_eq!(saved.lines().count(), 1);
        assert!(saved.contains("\"decision\":\"fix\""));
    }
}

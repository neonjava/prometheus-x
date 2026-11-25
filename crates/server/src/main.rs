mod batch;
mod responses;
mod state;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::map_response;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use batch::{BatchSettings, Batches};
use config::Config;
use detector::Catalog;
use responses::{LogResponse, PredictionResponse};
use serde::{Deserialize, Serialize};
use state::{bad_request, internal, product_choice, ApiError, AppState, Shared};
use tower_http::services::ServeDir;

const DECISIONS: [&str; 3] = ["ok", "reject", "fix"];
const MAX_DECISIONS_SHOWN: usize = 500;
// Multipart framing adds a little on top of the file bytes.
const MULTIPART_SLACK_BYTES: u64 = 1 << 20;

struct CheckRequest {
    name: String,
    bytes: Vec<u8>,
    product: Option<String>,
}

async fn read_check_request(mut multipart: Multipart) -> Result<CheckRequest, ApiError> {
    let mut image = None;
    let mut product = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(format!("invalid multipart: {e}")))?
    {
        let kind = field.name().map(str::to_string);
        match kind.as_deref() {
            Some("image") => {
                let name = field.file_name().unwrap_or("upload").to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| bad_request(format!("failed to read image: {e}")))?;
                image = Some((name, bytes.to_vec()));
            }
            Some("product") => {
                product = product_choice(field.text().await.map_err(bad_request)?);
            }
            _ => {}
        }
    }
    let (name, bytes) =
        image.ok_or_else(|| bad_request("missing image field in multipart upload"))?;
    Ok(CheckRequest {
        name,
        bytes,
        product,
    })
}

async fn predict(
    State(state): State<Shared>,
    multipart: Multipart,
) -> Result<Json<PredictionResponse>, ApiError> {
    let started = Instant::now();
    let CheckRequest {
        name,
        bytes,
        product,
    } = read_check_request(multipart).await?;
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

    if let Some(reason) = state.unusable(&image) {
        return Err(bad_request(reason));
    }

    // Inference is CPU-heavy, keep it off the async threads.
    let prediction =
        tokio::task::spawn_blocking(move || state.model.predict(&image, product.as_deref()))
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

fn read_decisions(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn read_decided_keys(path: &Path) -> std::collections::HashSet<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|v| v.get("image").and_then(|s| s.as_str()).map(String::from))
        .collect()
}

/// Saved decisions, newest first.
async fn decisions(State(state): State<Shared>) -> Json<Vec<serde_json::Value>> {
    let mut all = read_decisions(&state.decisions_path);
    all.reverse();
    all.truncate(MAX_DECISIONS_SHOWN);
    Json(all)
}

/// Photos needing human review that have not been decided yet.
async fn pending(State(state): State<Shared>) -> Json<Vec<batch::PendingItem>> {
    let decided = read_decided_keys(&state.decisions_path);
    Json(state.batches.pending_reviews(&decided))
}

#[derive(Serialize)]
struct ProductStatus {
    name: String,
    good_vectors: usize,
}

#[derive(Serialize)]
struct StatusResponse {
    /// Good photos and patches learned, summed over all products.
    good_vectors: usize,
    patch_rows: usize,
    products: Vec<ProductStatus>,
    reviews: usize,
    batches: usize,
    waiting_for_review: usize,
}

async fn status(State(state): State<Shared>) -> Json<StatusResponse> {
    let products = state.model.products();
    let decided = read_decided_keys(&state.decisions_path);
    let waiting_for_review = state.batches.pending_reviews(&decided).len();
    Json(StatusResponse {
        good_vectors: products.iter().map(|(_, i)| i.good_vectors).sum(),
        patch_rows: products.iter().map(|(_, i)| i.patch_rows).sum(),
        products: products
            .into_iter()
            .map(|(name, i)| ProductStatus {
                name,
                good_vectors: i.good_vectors,
            })
            .collect(),
        reviews: read_decisions(&state.decisions_path).len(),
        batches: state.batches.count(),
        waiting_for_review,
    })
}

// Browsers must re-check page files, or they keep showing an old version after an update.
async fn revalidate(mut response: Response) -> Response {
    response
        .headers_mut()
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-cache"));
    response
}

// Axum's default 2 MB body limit would reject real PCB photos.
fn app(state: Shared, web_dir: &Path, max_upload_bytes: usize, max_batch_bytes: u64) -> Router {
    let single = Router::new()
        .route("/predict", post(predict))
        .route("/decision", post(decision))
        .route("/decisions", get(decisions))
        .route("/review/pending", get(pending))
        .route("/status", get(status))
        .route("/batches", get(batch::list_endpoint))
        .route("/batch/{id}", get(batch::status).delete(batch::remove))
        .route("/batch/{id}/item/{name}", get(batch::item))
        .route("/batch/{id}/thumb/{name}", get(batch::thumb))
        .route("/batch/{id}/image/{name}", get(batch::original))
        .route("/batch/{id}/export.csv", get(batch::export_csv))
        .layer(DefaultBodyLimit::max(max_upload_bytes));
    let big = Router::new()
        .route("/batch", post(batch::create))
        .layer(DefaultBodyLimit::max(
            usize::try_from(max_batch_bytes + MULTIPART_SLACK_BYTES).unwrap_or(usize::MAX),
        ));
    single
        .merge(big)
        .fallback_service(ServeDir::new(web_dir))
        .layer(map_response(revalidate))
        .with_state(state)
}

fn config_path() -> PathBuf {
    // Allow overriding default config path via command line argument.
    match std::env::args().nth(1) {
        Some(arg) => PathBuf::from(arg),
        None => PathBuf::from("config.toml"),
    }
}

fn batch_settings(cfg: &Config) -> BatchSettings {
    BatchSettings {
        dir: cfg.batches_dir.clone(),
        max_bytes: cfg.max_batch_bytes,
        max_unpacked: cfg.max_unpacked_bytes,
        max_files: cfg.max_batch_files,
        workers: cfg.batch_workers,
        thumb: cfg.thumb_size,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::load(&config_path())?;
    let state: Shared = Arc::new(AppState {
        model: Box::new(Catalog::load(&cfg)?),
        decisions_path: cfg.decisions_path.clone(),
        batches: Batches::new(batch_settings(&cfg)),
        min_mean_brightness: cfg.min_mean_brightness,
    });
    let listener = tokio::net::TcpListener::bind(&cfg.server_bind)
        .await
        .map_err(|err| anyhow::anyhow!("failed to bind {}: {err}", cfg.server_bind))?;
    println!("server listening on {}", cfg.server_bind);
    let router = app(
        state,
        &cfg.web_dir,
        cfg.max_upload_bytes,
        cfg.max_batch_bytes,
    );
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::testing::StubModel;
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const TEST_UPLOAD_LIMIT: usize = 1 << 20;
    const BOUNDARY: &str = "------------------------boundary123";

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("px_server_{name}_{}", std::process::id()))
    }

    async fn start(name: &str) -> (std::net::SocketAddr, PathBuf) {
        let root = temp_path(name);
        let _ = std::fs::remove_dir_all(&root);
        let web = root.join("web");
        std::fs::create_dir_all(&web).expect("web dir");
        std::fs::write(web.join("index.html"), "<html>page</html>").expect("index");
        let batches = Batches::new(BatchSettings {
            dir: root.join("batches"),
            max_bytes: 1 << 30,
            max_unpacked: 1 << 30,
            max_files: 100,
            workers: 2,
            thumb: 16,
        });
        let state = Arc::new(AppState {
            model: Box::new(StubModel),
            min_mean_brightness: 1.0,
            decisions_path: root.join("decisions.jsonl"),
            batches,
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let router = app(state, &web, TEST_UPLOAD_LIMIT, 1 << 30);
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (addr, root)
    }

    async fn send(
        addr: std::net::SocketAddr,
        method: &str,
        path: &str,
        content_type: &str,
        body: &[u8],
    ) -> String {
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let mut stream = TcpStream::connect(addr).await.expect("connect to server");
        stream.write_all(head.as_bytes()).await.expect("send head");
        stream.write_all(body).await.expect("send body");
        // Thumbnails are binary, so read bytes and keep the readable part.
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("read response");
        String::from_utf8_lossy(&response).into_owned()
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

    fn form() -> String {
        format!("multipart/form-data; boundary={BOUNDARY}")
    }

    #[tokio::test]
    async fn predict_returns_staged_result_with_log() {
        let (addr, _) = start("a").await;
        let body = multipart("image", "test.png", &png_bytes());
        let response = send(addr, "POST", "/predict", &form(), &body).await;
        assert!(response.contains("200 OK"));
        assert!(response.contains("\"verdict\":\"Defect\""));
        assert!(response.contains("\"severity\":\"Medium\""));
        assert!(response.contains("Upload received: test.png"));
        assert!(response.contains("Image decoded: 16x16 px"));
        assert!(response.contains("stub step"));
    }

    #[tokio::test]
    async fn predict_rejects_missing_image_field() {
        let (addr, _) = start("b").await;
        let body = multipart("other", "x.txt", b"hello");
        let response = send(addr, "POST", "/predict", &form(), &body).await;
        assert!(response.contains("400 Bad Request"));
    }

    #[tokio::test]
    async fn decision_is_saved_listed_and_bad_values_are_rejected() {
        let (addr, root) = start("c").await;
        let good = br#"{"image":"a.jpg","verdict":"Uncertain","decision":"fix"}"#;
        let response = send(addr, "POST", "/decision", "application/json", good).await;
        assert!(response.contains("204 No Content"));

        let bad = br#"{"image":"a.jpg","verdict":"Uncertain","decision":"maybe"}"#;
        let response = send(addr, "POST", "/decision", "application/json", bad).await;
        assert!(response.contains("400 Bad Request"));

        let saved = std::fs::read_to_string(root.join("decisions.jsonl")).expect("decisions file");
        assert_eq!(saved.lines().count(), 1);
        let listed = send(addr, "GET", "/decisions", "application/json", b"").await;
        assert!(listed.contains("\"decision\":\"fix\""));
    }

    #[tokio::test]
    async fn status_and_web_files_are_served() {
        let (addr, _) = start("d").await;
        let response = send(addr, "GET", "/status", "application/json", b"").await;
        assert!(response.contains("\"good_vectors\":3"));
        assert!(response.contains("\"name\":\"stub\""));
        let page = send(addr, "GET", "/", "text/html", b"").await;
        assert!(page.contains("<html>page</html>"));
        assert!(page.contains("cache-control: no-cache"), "{page}");
    }

    #[tokio::test]
    async fn zip_upload_is_unpacked_and_checked() {
        let (addr, _) = start("f").await;
        let mut zip_bytes = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut zip_bytes);
            let options = zip::write::SimpleFileOptions::default();
            // Many entries so the archive is larger than one upload chunk.
            for n in 0..40 {
                writer
                    .start_file(format!("img{n}.png"), options)
                    .expect("start");
                std::io::Write::write_all(&mut writer, &png_bytes()).expect("write");
            }
            writer.finish().expect("finish");
        }
        let body = multipart("image", "set.zip", zip_bytes.get_ref());
        let response = send(addr, "POST", "/batch", &form(), &body).await;
        let id = response
            .split("\"batch_id\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("batch id")
            .to_string();
        let mut finished = String::new();
        for _ in 0..100 {
            finished = send(
                addr,
                "GET",
                &format!("/batch/{id}"),
                "application/json",
                b"",
            )
            .await;
            if finished.contains("\"status\":\"done\"")
                || finished.contains("\"status\":\"failed\"")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(finished.contains("\"status\":\"done\""), "{finished}");
        assert!(finished.contains("\"total\":40"), "{finished}");
        assert!(finished.contains("\"name\":\"img39.png\""));
    }

    #[tokio::test]
    async fn batch_of_images_is_processed_and_served_back() {
        let (addr, _) = start("e").await;
        let body = multipart("image", "one.png", &png_bytes());
        let response = send(addr, "POST", "/batch", &form(), &body).await;
        assert!(response.contains("200 OK"), "{response}");
        let id = response
            .split("\"batch_id\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("batch id")
            .to_string();

        let mut finished = String::new();
        for _ in 0..50 {
            finished = send(
                addr,
                "GET",
                &format!("/batch/{id}"),
                "application/json",
                b"",
            )
            .await;
            if finished.contains("\"status\":\"done\"") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(finished.contains("\"status\":\"done\""), "{finished}");
        assert!(finished.contains("\"name\":\"one.png\""));
        let thumb = send(
            addr,
            "GET",
            &format!("/batch/{id}/thumb/one.png"),
            "image/jpeg",
            b"",
        )
        .await;
        assert!(thumb.contains("200 OK"));
        let csv = send(
            addr,
            "GET",
            &format!("/batch/{id}/export.csv"),
            "text/csv",
            b"",
        )
        .await;
        assert!(csv.contains("one.png,stub,Defect"));
        let removed = send(
            addr,
            "DELETE",
            &format!("/batch/{id}"),
            "application/json",
            b"",
        )
        .await;
        assert!(removed.contains("204 No Content"), "{removed}");
        let gone = send(
            addr,
            "GET",
            &format!("/batch/{id}"),
            "application/json",
            b"",
        )
        .await;
        assert!(gone.contains("404"), "{gone}");

        let escape = send(
            addr,
            "GET",
            &format!("/batch/{id}/image/..%2Fsecret"),
            "image/png",
            b"",
        )
        .await;
        assert!(escape.contains("400") || escape.contains("404"));
    }

    #[tokio::test]
    async fn batches_and_pending_reviews_are_served() {
        let (addr, _) = start("g").await;
        let batches_resp = send(addr, "GET", "/batches", "application/json", b"").await;
        assert!(batches_resp.contains("200 OK"));
        assert!(batches_resp.contains("[]"));

        let pending_resp = send(addr, "GET", "/review/pending", "application/json", b"").await;
        assert!(pending_resp.contains("200 OK"));
        assert!(pending_resp.contains("[]"));
    }
}

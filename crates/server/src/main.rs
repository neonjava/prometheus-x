use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use config::Config;
use detector::{ModelResult, Predictor};
use image::DynamicImage;
use serde::{Deserialize, Serialize};

const INDEX_HTML: &str = include_str!("../../../web/index.html");

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct ModelResponse {
    score: f32,
    threshold: f32,
    vote: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct PredictionResponse {
    verdict: String,
    needs_review: bool,
    global: ModelResponse,
    patch: ModelResponse,
}

impl From<ModelResult> for ModelResponse {
    fn from(m: ModelResult) -> Self {
        Self {
            score: m.score,
            threshold: m.threshold,
            vote: m.vote.to_string(),
        }
    }
}

/// Anything that can judge an image; lets tests run without model weights.
trait Model: Send + Sync + 'static {
    fn predict(&self, image: &DynamicImage) -> Result<PredictionResponse, String>;
}

impl Model for Predictor {
    fn predict(&self, image: &DynamicImage) -> Result<PredictionResponse, String> {
        let prediction = Predictor::predict(self, image).map_err(|e| e.to_string())?;
        Ok(PredictionResponse {
            verdict: prediction.verdict.to_string(),
            needs_review: prediction.needs_review,
            global: prediction.global.into(),
            patch: prediction.patch.into(),
        })
    }
}

type Shared = Arc<dyn Model>;
type ApiError = (StatusCode, String);

fn bad_request(message: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, message.to_string())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn read_image_field(mut multipart: Multipart) -> Result<Vec<u8>, ApiError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(format!("invalid multipart: {e}")))?
    {
        if field.name() == Some("image") {
            let bytes = field
                .bytes()
                .await
                .map_err(|e| bad_request(format!("failed to read image: {e}")))?;
            return Ok(bytes.to_vec());
        }
    }
    Err(bad_request("missing image field in multipart upload"))
}

async fn predict(
    State(model): State<Shared>,
    multipart: Multipart,
) -> Result<Json<PredictionResponse>, ApiError> {
    let bytes = read_image_field(multipart).await?;
    let image = image::load_from_memory(&bytes)
        .map_err(|e| bad_request(format!("invalid image payload: {e}")))?;
    // Inference is CPU-heavy, keep it off the async threads.
    let result = tokio::task::spawn_blocking(move || model.predict(&image))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(result))
}

// Axum's default 2 MB body limit would reject real PCB photos.
fn app(model: Shared, max_upload_bytes: usize) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/predict", post(predict))
        .layer(DefaultBodyLimit::max(max_upload_bytes))
        .with_state(model)
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
    let model: Shared = Arc::new(Predictor::load(&cfg)?);
    let listener = tokio::net::TcpListener::bind(&cfg.server_bind)
        .await
        .map_err(|err| anyhow::anyhow!("failed to bind {}: {err}", cfg.server_bind))?;
    println!("server listening on {}", cfg.server_bind);
    axum::serve(listener, app(model, cfg.max_upload_bytes)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const TEST_UPLOAD_LIMIT: usize = 1 << 20;

    struct StubModel;

    impl Model for StubModel {
        fn predict(&self, _image: &DynamicImage) -> Result<PredictionResponse, String> {
            let model = || ModelResponse {
                score: 0.0,
                threshold: 1.0,
                vote: "Good".to_string(),
            };
            Ok(PredictionResponse {
                verdict: "Good".to_string(),
                needs_review: false,
                global: model(),
                patch: model(),
            })
        }
    }

    fn synthetic_png_bytes() -> Vec<u8> {
        let img = RgbImage::from_fn(16, 16, |x, y| Rgb([x as u8, y as u8, 128]));
        let mut buffer = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
            .expect("encode synthetic png");
        buffer
    }

    #[tokio::test]
    async fn predict_endpoint_returns_json_for_valid_image() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app(Arc::new(StubModel), TEST_UPLOAD_LIMIT)).await;
        });

        let png = synthetic_png_bytes();
        let boundary = "------------------------boundary123";
        let body_prefix = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"test.png\"\r\nContent-Type: image/png\r\n\r\n"
        );
        let body_suffix = format!("\r\n--{boundary}--\r\n");

        let mut body = Vec::new();
        body.extend_from_slice(body_prefix.as_bytes());
        body.extend_from_slice(&png);
        body.extend_from_slice(body_suffix.as_bytes());

        let request = format!(
            "POST /predict HTTP/1.1\r\nHost: {addr}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );

        let mut stream = TcpStream::connect(addr).await.expect("connect to server");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("send request headers");
        stream.write_all(&body).await.expect("send request body");

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("read response");

        assert!(response.contains("200 OK"));
        assert!(response.contains("\"verdict\":\"Good\",\"needs_review\":false"));
        assert!(response.contains("\"patch\":{\"score\":0.0,\"threshold\":1.0,\"vote\":\"Good\"}"));
    }

    #[tokio::test]
    async fn predict_endpoint_rejects_missing_image_field() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app(Arc::new(StubModel), TEST_UPLOAD_LIMIT)).await;
        });

        let boundary = "------------------------boundary456";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"other\"\r\n\r\nhello\r\n--{boundary}--\r\n"
        );

        let request = format!(
            "POST /predict HTTP/1.1\r\nHost: {addr}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        let mut stream = TcpStream::connect(addr).await.expect("connect to server");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("send request");

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("read response");

        assert!(response.contains("400 Bad Request"));
    }
}

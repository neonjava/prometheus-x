use std::path::{Path, PathBuf};

use axum::extract::{DefaultBodyLimit, Multipart};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Config {
    server_bind: String,
    max_upload_bytes: usize,
}

impl Config {
    // Read server bind address from config file.
    fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|err| anyhow::anyhow!("failed to read config {}: {err}", path.display()))?;
        let config: Self = toml::from_str(&content)
            .map_err(|err| anyhow::anyhow!("failed to parse config {}: {err}", path.display()))?;
        if config.server_bind.trim().is_empty() {
            anyhow::bail!("server_bind in {} must not be empty", path.display());
        }
        if config.max_upload_bytes == 0 {
            anyhow::bail!(
                "max_upload_bytes in {} must be greater than 0",
                path.display()
            );
        }
        Ok(config)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct PredictionResponse {
    verdict: String,
    score: f64,
}

// Extract image field from multipart upload and return prediction.
async fn predict(
    mut multipart: Multipart,
) -> Result<Json<PredictionResponse>, (StatusCode, String)> {
    let mut image_bytes = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| (StatusCode::BAD_REQUEST, format!("invalid multipart: {err}")))?
    {
        if field.name() == Some("image") {
            let bytes = field.bytes().await.map_err(|err| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("failed to read image: {err}"),
                )
            })?;
            image_bytes = Some(bytes);
            break;
        }
    }

    let bytes = match image_bytes {
        Some(b) => b,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                "missing image field in multipart upload".to_string(),
            ));
        }
    };

    // Reject unparseable image bytes before inference.
    image::load_from_memory(&bytes).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid image payload: {err}"),
        )
    })?;

    // Fixed mock until detector integration (#6) is wired.
    Ok(Json(PredictionResponse {
        verdict: "Good".to_string(),
        score: 0.0,
    }))
}

// Axum's default 2 MB body limit would reject real PCB photos.
fn app(max_upload_bytes: usize) -> Router {
    Router::new()
        .route("/predict", post(predict))
        .layer(DefaultBodyLimit::max(max_upload_bytes))
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
    let listener = tokio::net::TcpListener::bind(&cfg.server_bind)
        .await
        .map_err(|err| anyhow::anyhow!("failed to bind {}: {err}", cfg.server_bind))?;
    println!("server listening on {}", cfg.server_bind);
    axum::serve(listener, app(cfg.max_upload_bytes)).await?;
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

    struct TempConfig(PathBuf);

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn write_temp_config(content: &str) -> TempConfig {
        let path = std::env::temp_dir().join(format!(
            "prometheus_x_test_config_{}_{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, content).expect("write temp config");
        TempConfig(path)
    }

    fn synthetic_png_bytes() -> Vec<u8> {
        let img = RgbImage::from_fn(16, 16, |x, y| Rgb([x as u8, y as u8, 128]));
        let mut buffer = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
            .expect("encode synthetic png");
        buffer
    }

    #[test]
    fn config_load_valid() {
        let temp = write_temp_config("server_bind = \"127.0.0.1:8080\"\nmax_upload_bytes = 1024\n");
        let cfg = Config::load(&temp.0).expect("load config");
        assert_eq!(cfg.server_bind, "127.0.0.1:8080");
        assert_eq!(cfg.max_upload_bytes, 1024);
    }

    #[test]
    fn config_load_empty_bind_fails() {
        let temp = write_temp_config("server_bind = \"   \"\nmax_upload_bytes = 1024\n");
        assert!(Config::load(&temp.0).is_err());
    }

    #[test]
    fn config_load_zero_upload_limit_fails() {
        let temp = write_temp_config("server_bind = \"127.0.0.1:8080\"\nmax_upload_bytes = 0\n");
        assert!(Config::load(&temp.0).is_err());
    }

    #[test]
    fn config_load_missing_file_fails() {
        let missing = PathBuf::from("nonexistent_config_file_for_test.toml");
        assert!(Config::load(&missing).is_err());
    }

    #[tokio::test]
    async fn predict_endpoint_returns_json_for_valid_image() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app(TEST_UPLOAD_LIMIT)).await;
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
        assert!(response.contains("{\"verdict\":\"Good\",\"score\":0.0}"));
    }

    #[tokio::test]
    async fn predict_endpoint_rejects_missing_image_field() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app(TEST_UPLOAD_LIMIT)).await;
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

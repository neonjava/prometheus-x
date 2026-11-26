//! Camera mode: teach a product from frames, and relay a live view to other screens.
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use axum::extract::{Multipart, Path as UrlPath, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::responses::PredictionResponse;
use crate::state::{bad_request, internal, not_found, ApiError, Shared};

/// Longest product name accepted; it becomes a folder name.
const MAX_NAME_LEN: usize = 40;

/// The newest camera frame and the newest verdict, so another screen can follow along.
#[derive(Default)]
pub struct Live(Mutex<LiveData>);

#[derive(Default)]
struct LiveData {
    frame: Vec<u8>,
    frame_seq: u64,
    received: Option<Instant>,
    result: Option<PredictionResponse>,
    result_seq: u64,
}

impl Live {
    fn data(&self) -> std::sync::MutexGuard<'_, LiveData> {
        // Each update replaces whole fields, so a poisoned lock is still consistent.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Serialize)]
pub struct LiveStatus {
    frame_seq: u64,
    result_seq: u64,
    /// Milliseconds since the last frame, or none before the first one.
    age_ms: Option<u64>,
    result: Option<PredictionResponse>,
}

pub async fn live_status(State(state): State<Shared>) -> Json<LiveStatus> {
    let live = state.live.data();
    Json(LiveStatus {
        frame_seq: live.frame_seq,
        result_seq: live.result_seq,
        age_ms: live.received.map(|t| t.elapsed().as_millis() as u64),
        result: live.result.clone(),
    })
}

pub async fn live_frame(State(state): State<Shared>) -> Result<impl IntoResponse, ApiError> {
    let frame = state.live.data().frame.clone();
    if frame.is_empty() {
        return Err(not_found("no camera frame yet"));
    }
    Ok(([(header::CONTENT_TYPE, "image/jpeg")], frame))
}

pub async fn push_frame(
    State(state): State<Shared>,
    multipart: Multipart,
) -> Result<StatusCode, ApiError> {
    let frame = read_frames(multipart)
        .await?
        .pop()
        .ok_or_else(|| bad_request("missing image field"))?;
    let mut live = state.live.data();
    live.frame = frame;
    live.frame_seq += 1;
    live.received = Some(Instant::now());
    Ok(StatusCode::NO_CONTENT)
}

/// Remembers the latest verdict for watching screens.
pub fn publish_result(state: &Shared, result: &PredictionResponse) {
    let mut live = state.live.data();
    live.result = Some(result.clone());
    live.result_seq += 1;
}

#[derive(Debug, Serialize)]
pub struct Learned {
    name: String,
    good_frames: usize,
}

/// The name becomes a folder, so only simple characters are allowed.
fn valid_product_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

async fn read_frames(mut multipart: Multipart) -> Result<Vec<Vec<u8>>, ApiError> {
    let mut frames = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(bad_request)? {
        if field.name() == Some("image") {
            frames.push(field.bytes().await.map_err(bad_request)?.to_vec());
        }
    }
    Ok(frames)
}

// Replaces earlier frames, holding out every `val_every`-th one to set the limits.
fn save_frames(dir: &Path, frames: &[Vec<u8>], val_every: usize) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let (train, val) = (dir.join("train/good"), dir.join("val/good"));
    for folder in [&train, &val] {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    for (index, bytes) in frames.iter().enumerate() {
        image::load_from_memory(bytes)
            .map_err(|e| format!("frame {index} is not an image: {e}"))?;
        let held_out = index % val_every == val_every - 1;
        let folder = if held_out { &val } else { &train };
        std::fs::write(folder.join(format!("{index:04}.jpg")), bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn learn(
    State(state): State<Shared>,
    UrlPath(name): UrlPath<String>,
    multipart: Multipart,
) -> Result<Json<Learned>, ApiError> {
    if !valid_product_name(&name) {
        return Err(bad_request(format!(
            "product name may use a-z, 0-9, - and _ (up to {MAX_NAME_LEN} characters)"
        )));
    }
    let frames = read_frames(multipart).await?;
    if frames.len() < state.camera.min_frames {
        return Err(bad_request(format!(
            "need at least {} frames, got {}",
            state.camera.min_frames,
            frames.len()
        )));
    }
    // A dataset product's name must not even get its captured frames overwritten.
    let taken = state.model.products();
    if taken.iter().any(|(n, info)| *n == name && !info.camera) {
        return Err(bad_request(format!(
            "'{name}' is already a dataset product; choose another name"
        )));
    }
    // Decoding and model building are CPU-heavy, keep them off the async threads.
    tokio::task::spawn_blocking(move || {
        let dir = state.camera.dir.join(&name);
        save_frames(&dir, &frames, state.camera.val_every).map_err(bad_request)?;
        let info = state.model.learn(&name, &dir).map_err(bad_request)?;
        Ok(Json(Learned {
            name,
            good_frames: info.good_vectors,
        }))
    })
    .await
    .map_err(internal)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cannot_leave_the_folder() {
        assert!(valid_product_name("my-charger_2"));
        for bad in ["", "../x", "a/b", "A b", "x.y"] {
            assert!(!valid_product_name(bad), "{bad}");
        }
    }

    #[test]
    fn frames_are_split_and_junk_is_refused() {
        let dir = std::env::temp_dir().join(format!("px_camera_{}", std::process::id()));
        let mut png = Vec::new();
        image::RgbImage::new(4, 4)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        save_frames(&dir, &vec![png.clone(); 6], 3).expect("save");
        let count = |sub: &str| std::fs::read_dir(dir.join(sub)).expect("dir").count();
        assert_eq!((count("train/good"), count("val/good")), (4, 2));
        assert!(save_frames(&dir, &[b"junk".to_vec()], 3).is_err());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}

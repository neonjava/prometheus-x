//! Batch checking: upload many images or a zip, process them in the background,
//! and keep every result on disk so a refresh or restart loses nothing.
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use axum::extract::{Multipart, Path as UrlPath, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::responses::PredictionResponse;
use crate::state::{bad_request, internal, not_found, ApiError, Shared};

const RESULTS_FILE: &str = "results.jsonl";
const IMAGES_DIR: &str = "images";
const THUMBS_DIR: &str = "thumbs";
const INCOMING_DIR: &str = "incoming";

pub struct BatchSettings {
    pub dir: PathBuf,
    pub max_bytes: u64,
    pub max_unpacked: u64,
    pub max_files: usize,
    pub workers: usize,
    pub thumb: u32,
}

/// One image's outcome, stored as a line of `results.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    pub response: Option<PredictionResponse>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Summary {
    name: String,
    status: &'static str,
    error: Option<String>,
    verdict: Option<String>,
    needs_review: bool,
    global_score: Option<f32>,
    patch_score: Option<f32>,
    severity: Option<String>,
    defect_probability: Option<f32>,
    shift_warning: bool,
}

#[derive(Serialize)]
pub struct BatchStatus {
    status: String,
    total: usize,
    done: usize,
    error: Option<String>,
    items: Vec<Summary>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct BatchCounts {
    pub total: usize,
    pub done: usize,
    pub ok: usize,
    pub review: usize,
    pub defect: usize,
    pub error: usize,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct BatchListItem {
    pub id: String,
    pub time: u64,
    pub status: String,
    pub counts: BatchCounts,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct PendingItem {
    pub batch_id: String,
    pub batch_name: String,
    pub image: String,
    pub name: String,
    pub verdict: String,
    pub thumb_url: String,
    pub time_unix: u64,
    pub global_score: Option<f32>,
    pub patch_score: Option<f32>,
    pub severity: Option<String>,
    pub defect_probability: Option<f32>,
}

struct Job {
    status: Mutex<String>,
    error: Mutex<Option<String>>,
    total: AtomicUsize,
    done: AtomicUsize,
    records: Mutex<Vec<Record>>,
}

impl Job {
    fn new(status: &str) -> Self {
        Self {
            status: Mutex::new(status.to_string()),
            error: Mutex::new(None),
            total: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            records: Mutex::new(Vec::new()),
        }
    }

    fn set_status(&self, status: &str) {
        if let Ok(mut guard) = self.status.lock() {
            *guard = status.to_string();
        }
    }
}

pub struct Batches {
    settings: BatchSettings,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
    counter: AtomicUsize,
}

impl Batches {
    pub fn new(settings: BatchSettings) -> Self {
        Self {
            settings,
            jobs: Mutex::new(HashMap::new()),
            counter: AtomicUsize::new(0),
        }
    }

    pub fn count(&self) -> usize {
        fs::read_dir(&self.settings.dir)
            .map(|entries| entries.filter_map(Result::ok).count())
            .unwrap_or(0)
    }

    fn batch_dir(&self, id: &str) -> PathBuf {
        self.settings.dir.join(id)
    }

    fn new_id(&self) -> String {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!(
            "{seconds:x}-{:x}",
            self.counter.fetch_add(1, Ordering::Relaxed)
        )
    }

    // A job lives in memory while running; afterwards it is read back from disk.
    fn job(&self, id: &str) -> Option<Arc<Job>> {
        if let Some(job) = self.jobs.lock().ok()?.get(id) {
            return Some(job.clone());
        }
        let records = read_records(&self.batch_dir(id).join(RESULTS_FILE)).ok()?;
        let job = Job::new("done");
        job.total.store(records.len(), Ordering::Relaxed);
        job.done.store(records.len(), Ordering::Relaxed);
        *job.records.lock().ok()? = records;
        let job = Arc::new(job);
        // Parse each finished batch once; the status poll asks for all of them.
        self.jobs.lock().ok()?.insert(id.to_string(), job.clone());
        Some(job)
    }

    /// All known batches from memory and disk, newest first.
    pub fn list(&self) -> Vec<BatchListItem> {
        let mut items: Vec<BatchListItem> = self
            .ids()
            .iter()
            .filter_map(|id| self.describe(id))
            .collect();
        items.sort_by_key(|a| std::cmp::Reverse(a.time));
        items
    }

    fn ids(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.settings.dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|id| valid_id(id))
            .collect()
    }

    // A folder that is still uploading has no job and no results yet, so it is skipped.
    fn describe(&self, id: &str) -> Option<BatchListItem> {
        let job = self.job(id)?;
        let status = job.status.lock().ok()?.clone();
        let records = job.records.lock().ok()?;
        Some(BatchListItem {
            id: id.to_string(),
            time: created_at(id),
            status,
            counts: compute_counts(
                job.total.load(Ordering::Relaxed),
                job.done.load(Ordering::Relaxed),
                &records,
            ),
        })
    }

    /// Photos that need human review and have no saved decision yet.
    pub fn pending_reviews(
        &self,
        decided_keys: &std::collections::HashSet<String>,
    ) -> Vec<PendingItem> {
        let mut pending = Vec::new();
        for id in self.ids() {
            let Some(job) = self.job(&id) else { continue };
            let Ok(records) = job.records.lock() else {
                continue;
            };
            pending.extend(
                records
                    .iter()
                    .filter_map(|record| pending_item(&id, record))
                    .filter(|item| !decided_keys.contains(&item.image)),
            );
        }
        pending.sort_by_key(|a| std::cmp::Reverse(a.time_unix));
        pending
    }
}

/// Batch ids start with the creation time in hex seconds.
fn created_at(id: &str) -> u64 {
    id.split('-')
        .next()
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .unwrap_or(0)
}

fn pending_item(batch_id: &str, record: &Record) -> Option<PendingItem> {
    let response = record.response.as_ref().filter(|r| r.needs_review)?;
    let finding = response.finding.as_ref();
    Some(PendingItem {
        batch_id: batch_id.to_string(),
        batch_name: batch_id.to_string(),
        image: format!("{batch_id}/{}", record.name),
        name: record.name.clone(),
        verdict: response.verdict.clone(),
        thumb_url: format!("/batch/{batch_id}/thumb/{}", record.name),
        time_unix: created_at(batch_id),
        global_score: Some(response.global.score),
        patch_score: response.patch.as_ref().map(|p| p.score),
        severity: finding.map(|f| f.severity.clone()),
        defect_probability: finding.map(|f| f.defect_probability),
    })
}

fn compute_counts(total: usize, done: usize, records: &[Record]) -> BatchCounts {
    let mut ok = 0;
    let mut review = 0;
    let mut defect = 0;
    let mut error = 0;
    for r in records {
        if r.error.is_some() {
            error += 1;
        } else if let Some(resp) = &r.response {
            if resp.needs_review {
                review += 1;
            } else if resp.verdict == "Defect" {
                defect += 1;
            } else {
                ok += 1;
            }
        }
    }
    BatchCounts {
        total,
        done,
        ok,
        review,
        defect,
        error,
    }
}

fn read_records(path: &Path) -> io::Result<Vec<Record>> {
    let file = File::open(path)?;
    Ok(BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// A plain file name only: no folders, no `..`.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && Path::new(name).file_name().is_some_and(|f| f == name) && name != ".."
}

fn clean_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    base.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// Same name twice gets "-2", "-3" ... before the extension.
fn unique_name(dir: &Path, name: &str) -> String {
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let ext = path.extension().and_then(|e| e.to_str());
    let mut candidate = name.to_string();
    let mut n = 2;
    while dir.join(&candidate).exists() {
        candidate = match ext {
            Some(ext) => format!("{stem}-{n}.{ext}"),
            None => format!("{stem}-{n}"),
        };
        n += 1;
    }
    candidate
}

fn is_zip(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

struct Upload {
    name: String,
    path: PathBuf,
}

/// Counts of what is still allowed while unpacking one batch.
struct Budget {
    files_left: usize,
    bytes_left: u64,
}

fn unpack_zip(
    archive_path: &Path,
    images: &Path,
    budget: &mut Budget,
) -> Result<Vec<String>, String> {
    let file = File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("not a valid zip: {e}"))?;
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let path = entry
            .enclosed_name()
            .ok_or("zip contains an unsafe path (../ or absolute)")?;
        if !data::is_image_extension(&path) {
            continue;
        }
        if budget.files_left == 0 {
            return Err("zip has more images than the batch limit".into());
        }
        let raw = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
        let name = unique_name(images, &clean_name(raw));
        let mut out = File::create(images.join(&name)).map_err(|e| e.to_string())?;
        // Read one byte past the limit so an oversized entry is noticed, whatever size it declares.
        let written = io::copy(&mut (&mut entry).take(budget.bytes_left + 1), &mut out)
            .map_err(|e| e.to_string())?;
        if written > budget.bytes_left {
            return Err("zip unpacks to more than the allowed size".into());
        }
        budget.bytes_left -= written;
        budget.files_left -= 1;
        names.push(name);
    }
    Ok(names)
}

/// Turns the uploads into image files inside `images`; returns their names.
fn prepare_images(
    settings: &BatchSettings,
    images: &Path,
    uploads: &[Upload],
) -> Result<Vec<String>, String> {
    let mut budget = Budget {
        files_left: settings.max_files,
        bytes_left: settings.max_unpacked,
    };
    let mut names = Vec::new();
    for upload in uploads {
        if is_zip(&upload.name) {
            names.extend(unpack_zip(&upload.path, images, &mut budget)?);
        } else if data::is_image_extension(Path::new(&upload.name)) {
            if budget.files_left == 0 {
                return Err("more images than the batch limit".into());
            }
            let name = unique_name(images, &clean_name(&upload.name));
            fs::rename(&upload.path, images.join(&name)).map_err(|e| e.to_string())?;
            budget.files_left -= 1;
            names.push(name);
        }
    }
    if names.is_empty() {
        return Err("no images found in the upload".into());
    }
    Ok(names)
}

fn check_one(state: &Shared, dir: &Path, name: &str) -> Record {
    let fail = |error: String| Record {
        name: name.to_string(),
        response: None,
        error: Some(error),
    };
    let image = match image::open(dir.join(IMAGES_DIR).join(name)) {
        Ok(image) => image,
        Err(e) => return fail(format!("cannot read image: {e}")),
    };
    let prediction = match state.model.predict(&image) {
        Ok(prediction) => prediction,
        Err(e) => return fail(e),
    };
    let thumb = image.thumbnail(state.batches.settings.thumb, state.batches.settings.thumb);
    let thumb_path = dir.join(THUMBS_DIR).join(format!("{name}.jpg"));
    if let Err(e) = thumb.to_rgb8().save(&thumb_path) {
        return fail(format!("cannot save thumbnail: {e}"));
    }
    Record {
        name: name.to_string(),
        response: Some(prediction.into()),
        error: None,
    }
}

fn process(state: &Shared, job: &Job, dir: &Path, names: &[String]) -> Result<(), String> {
    job.total.store(names.len(), Ordering::Relaxed);
    job.set_status("running");
    let results = Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(RESULTS_FILE))
            .map_err(|e| e.to_string())?,
    );
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..state.batches.settings.workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(name) = names.get(index) else { break };
                let record = check_one(state, dir, name);
                if let (Ok(mut file), Ok(line)) = (results.lock(), serde_json::to_string(&record)) {
                    let _ = writeln!(file, "{line}");
                }
                if let Ok(mut records) = job.records.lock() {
                    records.push(record);
                }
                job.done.fetch_add(1, Ordering::Relaxed);
            });
        }
    });
    Ok(())
}

fn run_job(state: Shared, job: Arc<Job>, dir: PathBuf, uploads: Vec<Upload>) {
    let images = dir.join(IMAGES_DIR);
    let outcome = prepare_images(&state.batches.settings, &images, &uploads)
        .and_then(|names| process(&state, &job, &dir, &names));
    let _ = fs::remove_dir_all(dir.join(INCOMING_DIR));
    match outcome {
        Ok(()) => job.set_status("done"),
        Err(message) => {
            if let Ok(mut error) = job.error.lock() {
                *error = Some(message);
            }
            job.set_status("failed");
        }
    }
}

fn summarize(record: &Record) -> Summary {
    let response = record.response.as_ref();
    Summary {
        name: record.name.clone(),
        status: if record.error.is_some() {
            "error"
        } else {
            "ok"
        },
        error: record.error.clone(),
        verdict: response.map(|r| r.verdict.clone()),
        needs_review: response.is_some_and(|r| r.needs_review),
        global_score: response.map(|r| r.global.score),
        patch_score: response.and_then(|r| r.patch.as_ref()).map(|p| p.score),
        severity: response
            .and_then(|r| r.finding.as_ref())
            .map(|f| f.severity.clone()),
        defect_probability: response
            .and_then(|r| r.finding.as_ref())
            .map(|f| f.defect_probability),
        shift_warning: response.is_some_and(|r| r.shift.warning),
    }
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn to_csv(records: &[Record]) -> String {
    let mut out = String::from(
        "name,verdict,needs_review,global_score,patch_score,severity,defect_probability,shift_warning,error\n",
    );
    for record in records {
        let s = summarize(record);
        let number = |v: Option<f32>| v.map(|n| format!("{n:.4}")).unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            csv_field(&s.name),
            s.verdict.unwrap_or_default(),
            s.needs_review,
            number(s.global_score),
            number(s.patch_score),
            s.severity.unwrap_or_default(),
            number(s.defect_probability),
            s.shift_warning,
            csv_field(&s.error.unwrap_or_default()),
        ));
    }
    out
}

// ---- HTTP handlers -----------------------------------------------------

#[derive(Serialize)]
pub struct Created {
    batch_id: String,
}

/// Streams the upload to disk (never whole files in memory), then works in the background.
pub async fn create(
    State(state): State<Shared>,
    mut multipart: Multipart,
) -> Result<Json<Created>, ApiError> {
    let settings = &state.batches.settings;
    let id = state.batches.new_id();
    let dir = state.batches.batch_dir(&id);
    let incoming = dir.join(INCOMING_DIR);
    for sub in [IMAGES_DIR, THUMBS_DIR, INCOMING_DIR] {
        tokio::fs::create_dir_all(dir.join(sub))
            .await
            .map_err(internal)?;
    }

    let mut uploads = Vec::new();
    let mut total = 0u64;
    while let Some(mut field) = multipart.next_field().await.map_err(bad_request)? {
        let Some(name) = field.file_name().map(str::to_string) else {
            continue;
        };
        let path = incoming.join(format!("{}.part", uploads.len()));
        let mut file = tokio::fs::File::create(&path).await.map_err(internal)?;
        while let Some(chunk) = field.chunk().await.map_err(bad_request)? {
            total += chunk.len() as u64;
            if total > settings.max_bytes {
                let _ = tokio::fs::remove_dir_all(&dir).await;
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "upload is larger than the batch limit".into(),
                ));
            }
            file.write_all(&chunk).await.map_err(internal)?;
        }
        // tokio files buffer writes; without a flush the end of a big zip can be lost.
        file.flush().await.map_err(internal)?;
        uploads.push(Upload { name, path });
    }
    if uploads.is_empty() {
        let _ = tokio::fs::remove_dir_all(&dir).await;
        return Err(bad_request("no files in the upload"));
    }

    let job = Arc::new(Job::new("preparing"));
    state
        .batches
        .jobs
        .lock()
        .map_err(internal)?
        .insert(id.clone(), job.clone());
    let worker_state = state.clone();
    tokio::task::spawn_blocking(move || run_job(worker_state, job, dir, uploads));
    Ok(Json(Created { batch_id: id }))
}

fn find_job(state: &Shared, id: &str) -> Result<Arc<Job>, ApiError> {
    if !valid_id(id) {
        return Err(bad_request("bad batch id"));
    }
    state
        .batches
        .job(id)
        .ok_or_else(|| not_found("no such batch"))
}

pub async fn status(
    State(state): State<Shared>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<BatchStatus>, ApiError> {
    let job = find_job(&state, &id)?;
    let status = job.status.lock().map_err(internal)?.clone();
    let error = job.error.lock().map_err(internal)?.clone();
    let items = job
        .records
        .lock()
        .map_err(internal)?
        .iter()
        .map(summarize)
        .collect();
    Ok(Json(BatchStatus {
        status,
        total: job.total.load(Ordering::Relaxed),
        done: job.done.load(Ordering::Relaxed),
        error,
        items,
    }))
}

pub async fn list_endpoint(State(state): State<Shared>) -> Json<Vec<BatchListItem>> {
    Json(state.batches.list())
}

#[derive(Serialize)]
pub struct Item {
    name: String,
    response: Option<PredictionResponse>,
    error: Option<String>,
}

pub async fn item(
    State(state): State<Shared>,
    UrlPath((id, name)): UrlPath<(String, String)>,
) -> Result<Json<Item>, ApiError> {
    let job = find_job(&state, &id)?;
    let records = job.records.lock().map_err(internal)?;
    let record = records
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| not_found("no such image in this batch"))?;
    Ok(Json(Item {
        name: record.name.clone(),
        response: record.response.clone(),
        error: record.error.clone(),
    }))
}

async fn serve_file(
    path: PathBuf,
    content_type: &'static str,
) -> Result<impl IntoResponse, ApiError> {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| not_found("file not found"))?;
    Ok(([(header::CONTENT_TYPE, content_type)], bytes))
}

pub async fn thumb(
    State(state): State<Shared>,
    UrlPath((id, name)): UrlPath<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    if !valid_id(&id) || !valid_name(&name) {
        return Err(bad_request("bad name"));
    }
    let path = state
        .batches
        .batch_dir(&id)
        .join(THUMBS_DIR)
        .join(format!("{name}.jpg"));
    serve_file(path, "image/jpeg").await
}

pub async fn original(
    State(state): State<Shared>,
    UrlPath((id, name)): UrlPath<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    if !valid_id(&id) || !valid_name(&name) {
        return Err(bad_request("bad name"));
    }
    let content_type = match Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("bmp") => "image/bmp",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    serve_file(
        state.batches.batch_dir(&id).join(IMAGES_DIR).join(&name),
        content_type,
    )
    .await
}

pub async fn export_csv(
    State(state): State<Shared>,
    UrlPath(id): UrlPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    let job = find_job(&state, &id)?;
    let csv = to_csv(&job.records.lock().map_err(internal)?);
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"batch-{id}.csv\""),
            ),
        ],
        csv,
    ))
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;
    use crate::state::testing::StubModel;
    use crate::state::AppState;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("px_batch_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn settings(dir: &Path, max_files: usize, max_unpacked: u64) -> BatchSettings {
        BatchSettings {
            dir: dir.to_path_buf(),
            max_bytes: 1 << 30,
            max_unpacked,
            max_files,
            workers: 2,
            thumb: 16,
        }
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        RgbImage::from_fn(8, 8, |x, y| Rgb([x as u8, y as u8, 9]))
            .write_to(&mut io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode png");
        bytes
    }

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut writer = zip::ZipWriter::new(File::create(path).expect("create zip"));
        for (name, data) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("start file");
            writer.write_all(data).expect("write entry");
        }
        writer.finish().expect("finish zip");
    }

    #[test]
    fn zip_keeps_images_and_ignores_other_files() {
        let dir = temp_dir("keep");
        let archive = dir.join("a.zip");
        make_zip(
            &archive,
            &[
                ("one.png", &png()),
                ("sub/two.png", &png()),
                ("notes.txt", b"x"),
            ],
        );
        let images = dir.join("images");
        fs::create_dir_all(&images).expect("images dir");
        let mut budget = Budget {
            files_left: 10,
            bytes_left: 1 << 20,
        };
        let mut names = unpack_zip(&archive, &images, &mut budget).expect("unpack");
        names.sort();
        assert_eq!(names, vec!["one.png", "two.png"]);
        assert_eq!(budget.files_left, 8);
    }

    #[test]
    fn zip_with_escaping_path_is_rejected() {
        let dir = temp_dir("slip");
        let archive = dir.join("evil.zip");
        make_zip(&archive, &[("../evil.png", &png())]);
        let images = dir.join("images");
        fs::create_dir_all(&images).expect("images dir");
        let mut budget = Budget {
            files_left: 10,
            bytes_left: 1 << 20,
        };
        let error = unpack_zip(&archive, &images, &mut budget).expect_err("must reject");
        assert!(error.contains("unsafe path"));
        assert!(!dir.join("evil.png").exists());
    }

    #[test]
    fn zip_over_file_or_size_limit_is_rejected() {
        let dir = temp_dir("limits");
        let archive = dir.join("a.zip");
        make_zip(&archive, &[("a.png", &png()), ("b.png", &png())]);
        let images = dir.join("images");
        fs::create_dir_all(&images).expect("images dir");
        let mut few = Budget {
            files_left: 1,
            bytes_left: 1 << 20,
        };
        assert!(unpack_zip(&archive, &images, &mut few).is_err());
        let mut small = Budget {
            files_left: 10,
            bytes_left: 10,
        };
        assert!(unpack_zip(&archive, &images, &mut small).is_err());
    }

    #[test]
    fn duplicate_names_get_a_suffix() {
        let dir = temp_dir("names");
        fs::write(dir.join("a.png"), b"x").expect("write");
        assert_eq!(unique_name(&dir, "a.png"), "a-2.png");
        assert_eq!(clean_name("../my photo(1).jpg"), "my_photo_1_.jpg");
        assert!(valid_name("a.png") && !valid_name("../a.png") && !valid_name("a/b.png"));
    }

    #[test]
    fn batch_runs_to_the_end_and_one_bad_image_does_not_stop_it() {
        let dir = temp_dir("run");
        let batches = Batches::new(settings(&dir, 10, 1 << 20));
        let state: Shared = Arc::new(AppState {
            model: Box::new(StubModel),
            decisions_path: dir.join("d.jsonl"),
            batches,
        });
        let batch = dir.join("b1");
        for sub in [IMAGES_DIR, THUMBS_DIR] {
            fs::create_dir_all(batch.join(sub)).expect("dirs");
        }
        fs::write(batch.join(IMAGES_DIR).join("good.png"), png()).expect("write good");
        fs::write(batch.join(IMAGES_DIR).join("bad.png"), b"not an image").expect("write bad");

        let job = Job::new("preparing");
        let names = vec!["good.png".to_string(), "bad.png".to_string()];
        process(&state, &job, &batch, &names).expect("process");

        assert_eq!(job.done.load(Ordering::Relaxed), 2);
        let records = read_records(&batch.join(RESULTS_FILE)).expect("results file");
        assert_eq!(records.len(), 2);
        assert_eq!(records.iter().filter(|r| r.error.is_some()).count(), 1);
        assert!(batch.join(THUMBS_DIR).join("good.png.jpg").exists());
        let csv = to_csv(&records);
        assert!(csv.starts_with("name,verdict"));
        assert!(csv.contains("good.png,Defect,false"));
    }

    #[test]
    fn batches_list_and_pending_reviews() {
        use crate::responses::{ModelResponse, ShiftResponse};
        use std::collections::HashSet;

        let dir = temp_dir("list_pending");
        let batches = Batches::new(settings(&dir, 10, 1 << 20));
        let batch = dir.join("1000-0");
        fs::create_dir_all(&batch).expect("create batch dir");

        let p1 = PredictionResponse {
            verdict: "Good".to_string(),
            needs_review: false,
            global: ModelResponse {
                score: 1.0,
                threshold: 5.0,
                vote: "Good".to_string(),
            },
            patch: None,
            finding: None,
            shift: ShiftResponse {
                distance: 0.1,
                limit: 1.0,
                warning: false,
            },
            log: Vec::new(),
        };
        let p2 = PredictionResponse {
            verdict: "Uncertain".to_string(),
            needs_review: true,
            global: ModelResponse {
                score: 5.5,
                threshold: 5.0,
                vote: "Defect".to_string(),
            },
            patch: None,
            finding: None,
            shift: ShiftResponse {
                distance: 0.1,
                limit: 1.0,
                warning: false,
            },
            log: Vec::new(),
        };
        let r1 = Record {
            name: "img1.png".to_string(),
            response: Some(p1),
            error: None,
        };
        let r2 = Record {
            name: "img2.png".to_string(),
            response: Some(p2),
            error: None,
        };
        let mut file = File::create(batch.join(RESULTS_FILE)).expect("create results");
        writeln!(
            file,
            "{}",
            serde_json::to_string(&r1).expect("serialize r1")
        )
        .expect("write r1");
        writeln!(
            file,
            "{}",
            serde_json::to_string(&r2).expect("serialize r2")
        )
        .expect("write r2");

        let list = batches.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "1000-0");
        assert_eq!(list[0].counts.total, 2);
        assert_eq!(list[0].counts.ok, 1);
        assert_eq!(list[0].counts.review, 1);

        let mut decided = HashSet::new();
        let pending = batches.pending_reviews(&decided);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].image, "1000-0/img2.png");
        assert_eq!(pending[0].verdict, "Uncertain");

        decided.insert("1000-0/img2.png".to_string());
        let pending_after = batches.pending_reviews(&decided);
        assert_eq!(pending_after.len(), 0);
    }
}

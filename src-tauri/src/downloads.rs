//! Persistent, app-managed offline audio downloads.
//!
//! Downloads deliberately live outside mpv's cache. The latter is disposable and contains opaque
//! HTTP ranges; this module owns complete files, metadata and the queue that makes those files
//! usable after a restart or without a network connection.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use innertube::{AudioQuality, SongItem};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Mutex, Semaphore};

use crate::db::{now_secs, Db, DownloadCollectionRow, DownloadCollectionTrack, DownloadRow};
use crate::orchestrator::{Orchestrator, PlaybackData};

const PARENT_SETTING: &str = "download_parent";
const QUALITY_SETTING: &str = "download_quality";
const MANAGED_NAME: &str = "LiMusic Downloads";
const MARKER: &str = ".limusic-managed";
const AUDIO_DIR: &str = "audio";
const ART_DIR: &str = "artwork";
const CHUNK: u64 = 4 * 1024 * 1024;
const STALL: Duration = Duration::from_secs(20);
const RESERVE_BYTES: u64 = 64 * 1024 * 1024;

/// Open the desktop-native folder picker used for the download root.
///
/// `tauri-plugin-dialog` uses the GTK/rfd picker on Linux. Prefer KDE's `kdialog` in a
/// KDE/Plasma session so the chooser matches the rest of the desktop. The UI falls back to the
/// portable Tauri picker when kdialog is unavailable or when the app runs on another platform.
pub async fn pick_download_parent(
    initial: Option<String>,
    title: String,
) -> Result<Option<String>, String> {
    #[cfg(target_os = "linux")]
    {
        let is_kde = ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .any(|value| value.to_ascii_lowercase().contains("kde"));
        let is_plasma_session = std::env::var("KDE_FULL_SESSION")
            .map(|value| value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
            || std::env::var_os("KDE_SESSION_VERSION").is_some();
        if !is_kde && !is_plasma_session {
            return Err("KDE folder picker is not available in this desktop session".into());
        }

        let start_dir = initial
            .filter(|path| Path::new(path).is_dir())
            .or_else(|| std::env::var("HOME").ok().filter(|path| Path::new(path).is_dir()))
            .unwrap_or_else(|| ".".into());
        return tauri::async_runtime::spawn_blocking(move || {
            let output = std::process::Command::new("kdialog")
                .arg("--getexistingdirectory")
                .arg(&start_dir)
                .arg("--title")
                .arg(title)
                .output()
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        "KDE folder picker is not installed".to_owned()
                    } else {
                        format!("could not start KDE folder picker: {error}")
                    }
                })?;

            // kdialog exits non-zero when the user presses Cancel. That is a normal answer; a
            // launch error above lets the UI use its portable fallback instead.
            let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            Ok((output.status.success() && !path.is_empty()).then_some(path))
        })
        .await
        .map_err(|error| format!("KDE folder picker task failed: {error}"))?;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (initial, title);
        Err("KDE folder picker is only available on Linux".into())
    }
}

fn ensure_space(root: &Path, bytes: u64) -> Result<(), String> {
    let free = fs2::available_space(root).map_err(|e| format!("storage unavailable: {e}"))?;
    if free < bytes.saturating_add(RESERVE_BYTES) {
        return Err(format!("not enough free space ({} bytes available)", free));
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if !from.exists() {
        return Ok(());
    }
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else if src.is_file() {
            fs::copy(&src, &dst).map_err(|e| e.to_string())?;
            let src_len = fs::metadata(&src).map_err(|e| e.to_string())?.len();
            let dst_len = fs::metadata(&dst).map_err(|e| e.to_string())?.len();
            if src_len != dst_len {
                return Err(format!("download migration size check failed for {}", src.display()));
            }
        }
    }
    Ok(())
}

fn remove_empty_dirs(root: &Path) {
    for name in [AUDIO_DIR, ART_DIR] {
        let dir = root.join(name);
        let _ = fs::remove_dir(&dir);
    }
    let _ = fs::remove_file(root.join(MARKER));
    let _ = fs::remove_dir(root);
}

fn remove_orphans(root: &Path, allowed: &HashSet<String>) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            remove_orphans(&path, allowed);
            let _ = fs::remove_dir(&path);
        } else if path.is_file() {
            let rel = path.strip_prefix(root).ok().map(|p| p.to_string_lossy().replace('\\', "/"));
            if rel.as_deref() != Some(MARKER) && rel.as_ref().is_some_and(|p| !allowed.contains(p))
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub song: SongItem,
    pub state: String,
    pub quality: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub downloaded_bytes: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCollection {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub thumbnail: Option<String>,
    pub state: String,
    pub error: Option<String>,
    pub items: Vec<SongItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadLibrary {
    pub parent: Option<String>,
    pub managed_dir: Option<String>,
    pub storage_available: bool,
    pub bytes: i64,
    pub items: Vec<DownloadItem>,
    pub collections: Vec<DownloadCollection>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub video_id: String,
    pub state: String,
    pub downloaded_bytes: i64,
    pub size_bytes: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionRequest {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub thumbnail: Option<String>,
    pub items: Vec<SongItem>,
    pub continuation: Option<String>,
}

pub fn parent(db: &Db) -> Option<PathBuf> {
    db.get_setting(PARENT_SETTING).filter(|p| !p.trim().is_empty()).map(PathBuf::from)
}

pub fn managed_dir(db: &Db) -> Option<PathBuf> {
    parent(db).map(|p| p.join(MANAGED_NAME))
}

fn ext_for_mime(mime: Option<&str>) -> &'static str {
    let m = mime.unwrap_or_default();
    if m.contains("mp4") || m.contains("mp4a") {
        "m4a"
    } else {
        "webm"
    }
}

fn stable_name(video_id: &str) -> String {
    video_id.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

fn disabled_clients(db: &Db) -> HashSet<String> {
    let raw = std::env::var("LIMUSIC_DISABLED_CLIENTS")
        .ok()
        .or_else(|| db.get_setting("disabled_stream_clients"))
        .unwrap_or_default();
    raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

fn quality(db: &Db) -> AudioQuality {
    match db.get_setting(QUALITY_SETTING).as_deref() {
        Some("LOW") => AudioQuality::Low,
        _ => AudioQuality::High,
    }
}

fn quality_from_name(value: &str) -> AudioQuality {
    if value == "LOW" {
        AudioQuality::Low
    } else {
        AudioQuality::High
    }
}

fn quality_name(db: &Db) -> String {
    matches!(quality(db), AudioQuality::Low).then_some("LOW").unwrap_or("HIGH").to_owned()
}

fn path_for(db: &Db, relative: &str) -> Option<PathBuf> {
    let root = managed_dir(db)?;
    let p = root.join(relative);
    p.strip_prefix(&root)
        .ok()?
        .components()
        .all(|c| !matches!(c, std::path::Component::ParentDir))
        .then_some(p)
}

fn item_from_row(db: &Db, row: &DownloadRow) -> Option<SongItem> {
    let mut item: SongItem = serde_json::from_str(&row.song_json).ok()?;
    // Artwork paths are relative in the row. Resolve them at read time so a folder migration does
    // not require rewriting every SongItem JSON blob (and never leaves an old absolute path).
    if let Some(artwork) = row.artwork_path.as_deref().and_then(|p| path_for(db, p)) {
        if artwork.is_file() {
            item.thumbnail = Some(artwork.to_string_lossy().into_owned());
        }
    }
    Some(item)
}

fn item_view(db: &Db, row: DownloadRow) -> Option<DownloadItem> {
    Some(DownloadItem {
        song: item_from_row(db, &row)?,
        state: row.state,
        quality: row.quality,
        mime_type: row.mime_type,
        size_bytes: row.size_bytes,
        downloaded_bytes: row.downloaded_bytes,
        error: row.error,
    })
}

pub fn snapshot(db: &Db) -> DownloadLibrary {
    let items = db.list_downloads().into_iter().filter_map(|r| item_view(db, r)).collect();
    let collections = db
        .list_download_collections()
        .into_iter()
        .map(|c| DownloadCollection {
            id: c.id.clone(),
            kind: c.kind,
            title: c.title,
            subtitle: c.subtitle,
            thumbnail: c.thumbnail,
            state: c.state,
            error: c.error,
            items: db
                .download_collection_tracks(&c.id)
                .into_iter()
                .filter_map(|t| serde_json::from_str(&t.song_json).ok())
                .collect(),
        })
        .collect();
    let root = managed_dir(db);
    DownloadLibrary {
        parent: parent(db).map(|p| p.to_string_lossy().into_owned()),
        managed_dir: root.as_ref().map(|p| p.to_string_lossy().into_owned()),
        storage_available: root.as_ref().is_some_and(|p| p.is_dir()),
        bytes: db.download_bytes(),
        items,
        collections,
    }
}

pub fn emit_changed(app: &AppHandle, db: &Db) {
    let _ = app.emit("downloads-changed", snapshot(db));
}

pub fn allow_paths(app: &AppHandle, db: &Db) {
    let Some(dir) = managed_dir(db) else { return };
    let scope = app.asset_protocol_scope();
    let _ = scope.allow_directory(&dir, true);
    if let Ok(real) = dir.canonicalize() {
        let _ = scope.allow_directory(real, true);
    }
}

pub fn playback_data(db: &Db, video_id: &str) -> Option<PlaybackData> {
    let row = db.get_download(video_id)?;
    if row.state != "completed" {
        return None;
    }
    let path = path_for(db, &row.relative_path)?;
    if !path.is_file() {
        return None;
    }
    if row.size_bytes <= 0
        || fs::metadata(&path).ok()?.len() != row.size_bytes as u64
        || row.downloaded_bytes != row.size_bytes
    {
        return None;
    }
    let item = item_from_row(db, &row)?;
    Some(PlaybackData {
        video_id: video_id.to_owned(),
        stream_url: path.to_string_lossy().into_owned(),
        itag: 0,
        mime_type: row.mime_type,
        headers: Default::default(),
        expires_in_seconds: i64::MAX / 2,
        loudness_db: None,
        playback_ping: None,
        title: Some(item.title),
        artists: Some(item.artists),
        duration: item.duration,
        thumbnail: item.thumbnail,
        is_video: Some(false),
        stream_client: "download".to_owned(),
    })
}

fn content_range_total(v: Option<&str>) -> Option<u64> {
    v?.rsplit_once('/')?.1.trim().parse().ok()
}

async fn send_range(
    url: &str,
    headers: &std::collections::HashMap<String, String>,
    start: u64,
    end: u64,
) -> Result<reqwest::Response, String> {
    let mut req = crate::http::client()
        .get(url)
        .header(RANGE, format!("bytes={start}-{end}"))
        .header(ACCEPT_ENCODING, "identity");
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    tokio::time::timeout(STALL, req.send())
        .await
        .map_err(|_| "download request stalled".to_owned())?
        .map_err(|e| e.to_string())
}

pub struct DownloadManager {
    db: Arc<Db>,
    app: AppHandle,
    orchestrator: Arc<Orchestrator>,
    slots: Arc<Semaphore>,
    active: Arc<Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>>,
}

impl DownloadManager {
    pub fn new(db: Arc<Db>, app: AppHandle, orchestrator: Arc<Orchestrator>) -> Arc<Self> {
        let manager = Arc::new(Self {
            db,
            app,
            orchestrator,
            slots: Arc::new(Semaphore::new(2)),
            active: Arc::new(Mutex::new(std::collections::HashMap::new())),
        });
        if let Some(root) = managed_dir(&manager.db) {
            let rows = manager.db.list_downloads();
            let allowed: HashSet<String> = rows
                .iter()
                .flat_map(|r| [r.relative_path.clone(), r.part_path.clone()])
                .chain(rows.iter().filter_map(|r| r.artwork_path.clone()))
                .collect();
            remove_orphans(&root, &allowed);
        }
        // A process that died while writing cannot safely be called "downloading" on next boot.
        for row in manager.db.list_downloads() {
            if row.state == "downloading" || row.state == "resolving" {
                manager.db.update_download_progress(
                    &row.video_id,
                    "queued",
                    row.size_bytes,
                    row.downloaded_bytes,
                    None,
                    None,
                );
            }
        }
        manager
    }

    /// Resume only work that was intentionally queued. Rows paused by the user remain paused
    /// across restarts; interrupted workers are converted to queued in `new` above.
    pub fn resume_queued(self: &Arc<Self>) {
        for row in self.db.list_downloads() {
            if row.state == "queued" {
                let me = Arc::clone(self);
                let id = row.video_id;
                tauri::async_runtime::spawn(async move { me.run(id).await });
            }
        }
    }

    pub async fn enqueue(self: &Arc<Self>, items: Vec<SongItem>) {
        self.enqueue_with_mode(items, true).await;
    }

    async fn enqueue_with_mode(self: &Arc<Self>, items: Vec<SongItem>, standalone: bool) {
        for item in items {
            if item.video_id.starts_with("LOCAL:") {
                continue;
            }
            let id = item.video_id.clone();
            if let Some(row) = self.db.get_download(&id) {
                if standalone {
                    self.db.mark_download_standalone(&id);
                }
                if matches!(
                    row.state.as_str(),
                    "queued" | "resolving" | "downloading" | "completed"
                ) {
                    continue;
                }
            }
            let json = match serde_json::to_string(&item) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let name = stable_name(&id);
            let row = DownloadRow {
                video_id: id.clone(),
                song_json: json,
                relative_path: format!("{AUDIO_DIR}/{name}.webm"),
                part_path: format!("{AUDIO_DIR}/{name}.webm.part"),
                artwork_path: None,
                state: "queued".into(),
                quality: quality_name(&self.db),
                mime_type: None,
                size_bytes: 0,
                downloaded_bytes: 0,
                error: None,
                updated_at: now_secs(),
                standalone,
            };
            self.db.upsert_download(&row);
            let me = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                me.run(id).await;
            });
        }
        emit_changed(&self.app, &self.db);
    }

    pub async fn enqueue_collection(self: &Arc<Self>, req: CollectionRequest) {
        let collection = DownloadCollectionRow {
            id: req.id.clone(),
            kind: req.kind,
            title: req.title,
            subtitle: req.subtitle,
            thumbnail: req.thumbnail,
            continuation: req.continuation,
            state: "queued".into(),
            error: None,
        };
        self.db.upsert_download_collection(&collection);
        let tracks: Vec<_> = req
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                Some(DownloadCollectionTrack {
                    collection_id: req.id.clone(),
                    video_id: s.video_id.clone(),
                    position: i as i64,
                    song_json: serde_json::to_string(s).ok()?,
                })
            })
            .collect();
        self.db.set_download_collection_tracks(&req.id, &tracks);
        self.enqueue_with_mode(req.items, false).await;
        emit_changed(&self.app, &self.db);
    }

    async fn run(self: Arc<Self>, id: String) {
        let _slot = match self.slots.clone().acquire_owned().await {
            Ok(v) => v,
            Err(_) => return,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.active.lock().await.insert(id.clone(), cancel.clone());
        let mut result = self.download_one(&id, &cancel).await;
        // Signed googlevideo URLs can expire between resolving and the first Range request. One
        // fresh resolution is safe; repeated retries would turn an offline queue into a request
        // storm and are left for the explicit Retry action.
        if let Err(error) = &result {
            if error.contains("401") || error.contains("403") {
                result = self.download_one(&id, &cancel).await;
            }
        }
        self.active.lock().await.remove(&id);
        if let Err(e) = result {
            if let Some(row) = self.db.get_download(&id) {
                self.db.update_download_progress(
                    &id,
                    "failed",
                    row.size_bytes,
                    row.downloaded_bytes,
                    row.mime_type.as_deref(),
                    Some(&e),
                );
            }
        }
        emit_changed(&self.app, &self.db);
    }

    async fn download_one(&self, id: &str, cancel: &AtomicBool) -> Result<(), String> {
        let mut row = self.db.get_download(id).ok_or("download disappeared")?;
        let item: SongItem = serde_json::from_str(&row.song_json).map_err(|e| e.to_string())?;
        let root = managed_dir(&self.db).ok_or("choose a downloads folder first")?;
        fs::create_dir_all(root.join(AUDIO_DIR)).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join(ART_DIR)).map_err(|e| e.to_string())?;
        self.db.update_download_progress(
            id,
            "resolving",
            row.size_bytes,
            row.downloaded_bytes,
            None,
            None,
        );
        let data = self
            .orchestrator
            .resolve(
                id,
                item.is_upload,
                quality_from_name(&row.quality),
                &disabled_clients(&self.db),
            )
            .await
            .map_err(|e| e.to_string())?;
        let ext = ext_for_mime(data.mime_type.as_deref());
        let name = stable_name(id);
        let relative = format!("{AUDIO_DIR}/{name}.{ext}");
        let part_relative = format!("{AUDIO_DIR}/{name}.{ext}.part");
        if row.relative_path != relative {
            let old_final = root.join(&row.relative_path);
            let old_part = root.join(&row.part_path);
            row.relative_path = relative.clone();
            row.part_path = part_relative.clone();
            row.mime_type = data.mime_type.clone();
            row.downloaded_bytes = 0;
            row.size_bytes = 0;
            let _ = fs::remove_file(old_final);
            let _ = fs::remove_file(old_part);
            self.db.upsert_download(&row);
        }
        let final_path = root.join(&relative);
        let part_path = root.join(&part_relative);
        let mut existing = fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
        if final_path.is_file() && row.state == "completed" {
            return Ok(());
        }
        let meta = send_range(&data.stream_url, &data.headers, 0, 0).await?;
        let total =
            content_range_total(meta.headers().get(CONTENT_RANGE).and_then(|v| v.to_str().ok()))
                .or_else(|| {
                    meta.headers().get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.parse().ok())
                })
                .ok_or("stream did not report its size")?;
        if row.size_bytes > 0 && row.size_bytes as u64 != total {
            let _ = fs::remove_file(&part_path);
            existing = 0;
        }
        if existing > total {
            let _ = fs::remove_file(&part_path);
            existing = 0;
        }
        if existing > 0 && existing == total {
            fs::rename(&part_path, &final_path).map_err(|e| e.to_string())?;
            self.db.update_download_progress(
                id,
                "completed",
                total as i64,
                total as i64,
                data.mime_type.as_deref(),
                None,
            );
            return Ok(());
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&part_path)
            .map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(existing)).map_err(|e| e.to_string())?;
        self.db.update_download_progress(
            id,
            "downloading",
            total as i64,
            existing as i64,
            data.mime_type.as_deref(),
            None,
        );
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        while existing < total {
            if cancel.load(Ordering::Relaxed) {
                self.db.update_download_progress(
                    id,
                    "paused",
                    total as i64,
                    existing as i64,
                    data.mime_type.as_deref(),
                    None,
                );
                return Ok(());
            }
            let end = (existing + CHUNK - 1).min(total - 1);
            ensure_space(&root, end.saturating_sub(existing).saturating_add(1))?;
            let response = send_range(&data.stream_url, &data.headers, existing, end).await?;
            if !(response.status() == reqwest::StatusCode::PARTIAL_CONTENT
                || (existing == 0 && response.status().is_success()))
            {
                return Err(format!("download server returned {}", response.status()));
            }
            let mut stream = response.bytes_stream();
            while let Some(chunk) = tokio::time::timeout(STALL, stream.next())
                .await
                .map_err(|_| "download response stalled".to_owned())?
            {
                let bytes = chunk.map_err(|e| e.to_string())?;
                if cancel.load(Ordering::Relaxed) {
                    self.db.update_download_progress(
                        id,
                        "paused",
                        total as i64,
                        existing as i64,
                        data.mime_type.as_deref(),
                        None,
                    );
                    return Ok(());
                }
                file.write_all(&bytes).map_err(|e| e.to_string())?;
                existing += bytes.len() as u64;
                if last_emit.elapsed() >= Duration::from_millis(250) {
                    self.db.update_download_progress(
                        id,
                        "downloading",
                        total as i64,
                        existing as i64,
                        data.mime_type.as_deref(),
                        None,
                    );
                    let _ = self.app.emit(
                        "download-progress",
                        DownloadProgress {
                            video_id: id.to_owned(),
                            state: "downloading".into(),
                            downloaded_bytes: existing as i64,
                            size_bytes: total as i64,
                            error: None,
                        },
                    );
                    last_emit = Instant::now();
                }
            }
            if existing <= end {
                return Err("download ended before the requested range".into());
            }
        }
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&part_path, &final_path).map_err(|e| e.to_string())?;
        let mut final_item = item;
        if let Some(url) = final_item.thumbnail.as_deref() {
            if let Ok(resp) = crate::http::client().get(url).send().await {
                if let Ok(bytes) = resp.bytes().await {
                    let art_rel = format!("{ART_DIR}/{}.jpg", stable_name(id));
                    if fs::write(root.join(&art_rel), &bytes).is_ok() {
                        final_item.thumbnail =
                            Some(root.join(&art_rel).to_string_lossy().into_owned());
                        row.artwork_path = Some(art_rel);
                    }
                }
            }
        }
        row.song_json = serde_json::to_string(&final_item).map_err(|e| e.to_string())?;
        row.state = "completed".into();
        row.size_bytes = total as i64;
        row.downloaded_bytes = total as i64;
        row.error = None;
        row.mime_type = data.mime_type;
        self.db.upsert_download(&row);
        let _ = self.app.emit(
            "download-progress",
            DownloadProgress {
                video_id: id.to_owned(),
                state: "completed".into(),
                downloaded_bytes: total as i64,
                size_bytes: total as i64,
                error: None,
            },
        );
        Ok(())
    }

    pub async fn cancel(&self, id: &str) {
        if let Some(c) = self.active.lock().await.get(id) {
            c.store(true, Ordering::Relaxed);
        }
        if let Some(row) = self.db.get_download(id) {
            if let Some(p) = path_for(&self.db, &row.part_path) {
                let _ = fs::remove_file(p);
            }
            if let Some(p) = path_for(&self.db, &row.relative_path) {
                let _ = fs::remove_file(p);
            }
            self.db.delete_download(id);
        }
        emit_changed(&self.app, &self.db);
    }

    pub async fn pause(&self, id: &str) {
        if let Some(c) = self.active.lock().await.get(id) {
            c.store(true, Ordering::Relaxed);
        }
        if let Some(row) = self.db.get_download(id) {
            self.db.update_download_progress(
                id,
                "paused",
                row.size_bytes,
                row.downloaded_bytes,
                row.mime_type.as_deref(),
                None,
            );
        }
        emit_changed(&self.app, &self.db);
    }

    pub async fn retry(self: &Arc<Self>, id: &str) {
        while self.active.lock().await.contains_key(id) {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if let Some(row) = self.db.get_download(id) {
            if row.state == "completed" {
                if let Some(path) = path_for(&self.db, &row.relative_path) {
                    let _ = fs::remove_file(path);
                }
                if let Some(path) = path_for(&self.db, &row.part_path) {
                    let _ = fs::remove_file(path);
                }
                self.db.update_download_progress(
                    id,
                    "queued",
                    0,
                    0,
                    row.mime_type.as_deref(),
                    None,
                );
            } else {
                self.db.update_download_progress(
                    id,
                    "queued",
                    row.size_bytes,
                    row.downloaded_bytes,
                    row.mime_type.as_deref(),
                    None,
                );
            }
            let me = Arc::clone(self);
            let id = id.to_owned();
            tauri::async_runtime::spawn(async move {
                me.run(id).await;
            });
        }
        emit_changed(&self.app, &self.db);
    }

    pub async fn remove(&self, id: &str) {
        self.cancel(id).await;
    }

    pub async fn remove_collection(&self, collection_id: &str) {
        let tracks = self.db.download_collection_tracks(collection_id);
        self.db.delete_download_collection(collection_id);
        for track in tracks {
            if self.db.download_collection_count(&track.video_id) == 0
                && self.db.get_download(&track.video_id).is_some_and(|row| !row.standalone)
            {
                self.cancel(&track.video_id).await;
            }
        }
        emit_changed(&self.app, &self.db);
    }

    pub async fn clear(&self) {
        for cancel in self.active.lock().await.values() {
            cancel.store(true, Ordering::Relaxed);
        }
        while !self.active.lock().await.is_empty() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let rows = self.db.list_downloads();
        for row in rows {
            if let Some(p) = path_for(&self.db, &row.relative_path) {
                let _ = fs::remove_file(p);
            }
            if let Some(p) = path_for(&self.db, &row.part_path) {
                let _ = fs::remove_file(p);
            }
        }
        if let Some(root) = managed_dir(&self.db) {
            let _ = fs::remove_dir_all(root.join(ART_DIR));
            let _ = fs::remove_dir_all(root.join(AUDIO_DIR));
        }
        self.db.clear_downloads();
        emit_changed(&self.app, &self.db);
    }

    pub async fn set_parent(self: &Arc<Self>, parent_path: &str) -> Result<(), String> {
        let p = Path::new(parent_path);
        if !p.is_dir() {
            return Err("selected downloads parent is not a directory".into());
        }
        if parent(&self.db) == Some(p.to_path_buf()) {
            allow_paths(&self.app, &self.db);
            return Ok(());
        }
        // Stop workers before copying. Their cancellation path closes the partial file and removes
        // itself from `active`, so the copy below sees a stable byte count.
        for cancel in self.active.lock().await.values() {
            cancel.store(true, Ordering::Relaxed);
        }
        while !self.active.lock().await.is_empty() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let old_root = managed_dir(&self.db);
        let target = p.join(MANAGED_NAME);
        if target.exists() {
            // Never merge into an arbitrary existing directory: a partial migration must leave the
            // current library usable and must not overwrite someone else's files.
            return Err("the selected folder already contains LiMusic Downloads".into());
        }
        let temp = p.join(format!(".{MANAGED_NAME}.migration-{}", now_secs()));
        let _ = fs::remove_dir_all(&temp);
        if let Err(error) = (|| {
            if let Some(old) = old_root.as_deref() {
                copy_tree(old, &temp)?;
            }
            fs::create_dir_all(temp.join(AUDIO_DIR)).map_err(|e| e.to_string())?;
            fs::create_dir_all(temp.join(ART_DIR)).map_err(|e| e.to_string())?;
            fs::write(temp.join(MARKER), b"LiMusic Downloads\n").map_err(|e| e.to_string())?;
            fs::rename(&temp, &target).map_err(|e| e.to_string())
        })() {
            let _ = fs::remove_dir_all(&temp);
            return Err(format!("download migration failed: {error}"));
        }
        self.db.set_setting(PARENT_SETTING, parent_path);
        if let Some(old) = old_root {
            for row in self.db.list_downloads() {
                let _ = fs::remove_file(old.join(&row.relative_path));
                let _ = fs::remove_file(old.join(&row.part_path));
                if let Some(art) = row.artwork_path {
                    let _ = fs::remove_file(old.join(art));
                }
            }
            remove_empty_dirs(&old);
        }
        allow_paths(&self.app, &self.db);
        self.resume_queued();
        emit_changed(&self.app, &self.db);
        Ok(())
    }
}

pub fn download_quality(db: &Db) -> String {
    quality_name(db)
}
pub fn set_download_quality(db: &Db, value: &str) -> Result<(), String> {
    if !matches!(value, "LOW" | "HIGH") {
        return Err("download quality must be LOW or HIGH".into());
    }
    db.set_setting(QUALITY_SETTING, value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_names_are_filesystem_safe_and_deterministic() {
        assert_eq!(stable_name("abc"), "616263");
        assert_eq!(stable_name("abc"), stable_name("abc"));
        assert_ne!(stable_name("abc"), stable_name("abd"));
    }

    #[test]
    fn container_extensions_follow_the_original_stream() {
        assert_eq!(ext_for_mime(Some("audio/mp4; codecs=mp4a.40.2")), "m4a");
        assert_eq!(ext_for_mime(Some("audio/webm; codecs=opus")), "webm");
        assert_eq!(ext_for_mime(None), "webm");
    }

    #[test]
    fn content_range_reports_the_total_not_the_chunk_size() {
        assert_eq!(content_range_total(Some("bytes 0-3/42")), Some(42));
        assert_eq!(content_range_total(Some("bytes */42")), Some(42));
        assert_eq!(content_range_total(Some("invalid")), None);
    }

    #[test]
    fn playback_requires_a_complete_file_and_falls_back_when_it_is_missing() {
        let root = std::env::temp_dir().join(format!("limusic-download-test-{}", now_secs()));
        let managed = root.join(MANAGED_NAME).join(AUDIO_DIR);
        fs::create_dir_all(&managed).unwrap();
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        db.set_setting(PARENT_SETTING, root.to_str().unwrap());
        let name = stable_name("v1");
        let path = managed.join(format!("{name}.webm"));
        fs::write(&path, b"audio").unwrap();
        db.upsert_download(&DownloadRow {
            video_id: "v1".into(),
            song_json: r#"{"video_id":"v1","title":"T","artists":"A"}"#.into(),
            relative_path: format!("{AUDIO_DIR}/{name}.webm"),
            part_path: format!("{AUDIO_DIR}/{name}.webm.part"),
            artwork_path: None,
            state: "completed".into(),
            quality: "HIGH".into(),
            mime_type: Some("audio/webm".into()),
            size_bytes: 5,
            downloaded_bytes: 5,
            error: None,
            updated_at: now_secs(),
            standalone: true,
        });
        assert!(playback_data(&db, "v1").is_some());
        fs::remove_file(path).unwrap();
        assert!(playback_data(&db, "v1").is_none());
        let _ = fs::remove_dir_all(root);
    }
}

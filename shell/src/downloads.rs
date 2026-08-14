use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use reqwest::{
    blocking::Client,
    header::{HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::skip_segments::SkipSegment;

const MAX_ACTIVE_DOWNLOADS: usize = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub content_id: String,
    pub content_type: String,
    pub video_id: String,
    pub title: String,
    pub show_name: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub url: String,
    #[serde(default)]
    pub request_headers: HashMap<String, String>,
    #[serde(default)]
    pub source_name: String,
    pub filename: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: String,
    pub content_id: String,
    pub content_type: String,
    pub video_id: String,
    pub title: String,
    pub show_name: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub source_name: String,
    pub status: String,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub file_path: Option<String>,
    pub play_url: Option<String>,
    pub artwork_cached: bool,
    pub error: Option<String>,
    pub created_at: u64,
    pub skip_segments: Vec<SkipSegment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DownloadRecord {
    id: String,
    request: DownloadRequest,
    status: String,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
    file_path: Option<PathBuf>,
    artwork_path: Option<PathBuf>,
    error: Option<String>,
    created_at: u64,
    #[serde(default)]
    skip_segments: Vec<SkipSegment>,
}

#[derive(Debug)]
pub struct DownloadManager {
    root: PathBuf,
    records: Vec<DownloadRecord>,
    active_count: usize,
    cancelled: HashSet<String>,
}

impl Default for DownloadManager {
    fn default() -> Self {
        let root = load_root().unwrap_or_else(default_download_root);
        let mut records = load_records().unwrap_or_default();
        for record in &mut records {
            if matches!(record.status.as_str(), "downloading" | "queued") {
                record.status = "queued".to_string();
                record.bytes_downloaded = 0;
            }
        }
        Self {
            root,
            records,
            active_count: 0,
            cancelled: HashSet::new(),
        }
    }
}

impl DownloadManager {
    pub fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    pub fn items(&self) -> Vec<DownloadItem> {
        let mut items = self.records.iter().map(to_item).collect::<Vec<_>>();
        items.sort_by_key(|item| std::cmp::Reverse(item.created_at));
        items
    }

    pub fn artwork(&self, id: &str) -> Result<Option<(Vec<u8>, &'static str)>> {
        let Some(path) = self
            .records
            .iter()
            .find(|record| record.id == id)
            .and_then(|record| record.artwork_path.as_ref())
        else {
            return Ok(None);
        };
        ensure!(
            path.starts_with(&self.root),
            "Artwork is outside download storage"
        );
        let mime = match path.extension().and_then(|value| value.to_str()) {
            Some(value) if value.eq_ignore_ascii_case("png") => "image/png",
            Some(value) if value.eq_ignore_ascii_case("webp") => "image/webp",
            _ => "image/jpeg",
        };
        Ok(Some((fs::read(path)?, mime)))
    }

    pub fn cached_segments(
        &self,
        content_id: &str,
        video_id: &str,
        season: i64,
        episode: i64,
    ) -> Option<Vec<SkipSegment>> {
        self.records
            .iter()
            .find(|record| {
                record.request.content_id == content_id
                    && record.request.video_id == video_id
                    && record.request.season == Some(season)
                    && record.request.episode == Some(episode)
                    && record.status == "completed"
            })
            .map(|record| record.skip_segments.clone())
    }

    pub fn contains_play_url(&self, value: &str) -> bool {
        let Ok(url) = Url::parse(value) else {
            return false;
        };
        let Ok(path) = url.to_file_path() else {
            return false;
        };
        self.records.iter().any(|record| {
            record.status == "completed"
                && record
                    .file_path
                    .as_ref()
                    .is_some_and(|saved| saved == &path)
                && path.starts_with(&self.root)
        })
    }

    pub fn cancel(&mut self, id: &str) -> Result<()> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .context("Download not found")?;
        ensure!(
            matches!(record.status.as_str(), "queued" | "downloading"),
            "Only queued or active downloads can be cancelled"
        );
        record.status = "cancelled".to_string();
        record.error = None;
        self.cancelled.insert(id.to_string());
        self.persist()
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        let index = self
            .records
            .iter()
            .position(|record| record.id == id)
            .context("Download not found")?;
        ensure!(
            self.records[index].status != "downloading",
            "Cancel this download before removing it"
        );
        let record = self.records.remove(index);
        if let Some(path) = record.file_path
            && path.starts_with(&self.root)
            && path.is_file()
        {
            fs::remove_file(path)?;
        }
        self.persist()
    }

    pub fn retry(&mut self, id: &str) -> Result<()> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .context("Download not found")?;
        ensure!(
            matches!(record.status.as_str(), "failed" | "cancelled"),
            "Only failed or cancelled downloads can be retried"
        );
        record.status = "queued".to_string();
        record.bytes_downloaded = 0;
        record.total_bytes = None;
        record.error = None;
        self.cancelled.remove(id);
        self.persist()
    }

    pub fn move_storage(&mut self, selected: &Path) -> Result<()> {
        ensure!(selected.is_absolute(), "Choose an absolute download folder");
        ensure!(
            selected.components().count() > 1,
            "A drive or filesystem root cannot be used directly"
        );
        ensure!(
            self.active_count == 0 && !self.records.iter().any(|record| record.status == "queued"),
            "Wait for downloads to finish or cancel them before moving storage"
        );
        fs::create_dir_all(selected).context("Could not create the new download folder")?;
        let old_root = self.root.clone();
        if old_root == selected {
            return Ok(());
        }
        ensure!(
            !selected.starts_with(&old_root),
            "The new download folder cannot be inside the current one"
        );

        let mut moved_artwork = HashMap::<PathBuf, PathBuf>::new();
        for record in &mut self.records {
            if let Some(path) = record.file_path.clone()
                && path.starts_with(&old_root)
                && path.exists()
            {
                let target = selected.join(path.strip_prefix(&old_root)?);
                move_file(&path, &target)?;
                record.file_path = Some(target);
            }
            if let Some(path) = record.artwork_path.clone()
                && path.starts_with(&old_root)
            {
                let target = if let Some(existing) = moved_artwork.get(&path) {
                    Some(existing.clone())
                } else if path.exists() {
                    let target = selected.join(path.strip_prefix(&old_root)?);
                    move_file(&path, &target)?;
                    moved_artwork.insert(path.clone(), target.clone());
                    Some(target)
                } else {
                    None
                };
                if let Some(target) = target {
                    record.artwork_path = Some(target);
                }
            }
        }
        self.root = selected.to_path_buf();
        save_root(&self.root)?;
        self.persist()
    }

    fn persist(&self) -> Result<()> {
        let path = records_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(&self.records)?)?;
        Ok(())
    }
}

pub fn enqueue(
    manager: &Arc<Mutex<DownloadManager>>,
    request: DownloadRequest,
) -> Result<DownloadItem> {
    validate_request(&request)?;
    let item = {
        let mut state = manager
            .lock()
            .map_err(|_| anyhow::anyhow!("Download manager is unavailable"))?;
        if let Some(existing) = state.records.iter().find(|record| {
            record.request.content_id == request.content_id
                && record.request.video_id == request.video_id
                && record.status != "failed"
                && record.status != "cancelled"
        }) {
            return Ok(to_item(existing));
        }
        let record = DownloadRecord {
            id: Uuid::new_v4().to_string(),
            request,
            status: "queued".to_string(),
            bytes_downloaded: 0,
            total_bytes: None,
            file_path: None,
            artwork_path: None,
            error: None,
            created_at: now_secs(),
            skip_segments: Vec::new(),
        };
        let item = to_item(&record);
        state.records.push(record);
        state.persist()?;
        item
    };
    pump(Arc::clone(manager));
    Ok(item)
}

pub fn retry(manager: &Arc<Mutex<DownloadManager>>, id: &str) -> Result<()> {
    manager
        .lock()
        .map_err(|_| anyhow::anyhow!("Download manager is unavailable"))?
        .retry(id)?;
    pump(Arc::clone(manager));
    Ok(())
}

pub fn resume_queue(manager: &Arc<Mutex<DownloadManager>>) {
    pump(Arc::clone(manager));
}

fn pump(manager: Arc<Mutex<DownloadManager>>) {
    loop {
        let next = {
            let Ok(mut state) = manager.lock() else {
                return;
            };
            if state.active_count >= MAX_ACTIVE_DOWNLOADS {
                return;
            }
            let Some(record) = state
                .records
                .iter_mut()
                .find(|record| record.status == "queued")
            else {
                return;
            };
            record.status = "downloading".to_string();
            record.error = None;
            let id = record.id.clone();
            state.active_count += 1;
            let _ = state.persist();
            id
        };
        let shared = Arc::clone(&manager);
        thread::spawn(move || run_download(shared, next));
    }
}

fn run_download(manager: Arc<Mutex<DownloadManager>>, id: String) {
    let result = perform_download(&manager, &id);
    if let Ok(mut state) = manager.lock() {
        let cancelled = state.cancelled.remove(&id);
        if let Some(record) = state.records.iter_mut().find(|record| record.id == id) {
            if cancelled || record.status == "cancelled" {
                record.status = "cancelled".to_string();
            } else if let Err(error) = result {
                record.status = "failed".to_string();
                record.error = Some(format!("{error:#}"));
                if let Some(path) = record.file_path.as_ref()
                    && let Some(extension) = path.extension().and_then(|value| value.to_str())
                {
                    let _ = fs::remove_file(path.with_extension(format!("{extension}.part")));
                }
            } else {
                record.status = "completed".to_string();
                record.error = None;
            }
        }
        state.active_count = state.active_count.saturating_sub(1);
        let _ = state.persist();
    }
    pump(manager);
}

fn perform_download(manager: &Arc<Mutex<DownloadManager>>, id: &str) -> Result<()> {
    let (request, root) = {
        let state = manager
            .lock()
            .map_err(|_| anyhow::anyhow!("Download manager is unavailable"))?;
        let record = state
            .records
            .iter()
            .find(|record| record.id == id)
            .context("Download disappeared")?;
        (record.request.clone(), state.root.clone())
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("NuvioDesktop/0.1")
        .build()?;

    let skip_segments = match (request.season, request.episode) {
        (Some(season), Some(episode)) => {
            crate::skip_segments::resolve(&request.content_id, &request.video_id, season, episode)
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };

    let artwork_path = cache_artwork(&client, &root, &request).ok().flatten();
    {
        let mut state = manager
            .lock()
            .map_err(|_| anyhow::anyhow!("Download manager is unavailable"))?;
        if let Some(record) = state.records.iter_mut().find(|record| record.id == id) {
            record.skip_segments = skip_segments;
            if artwork_path.is_some() {
                record.artwork_path = artwork_path;
            }
        }
    }

    let source = Url::parse(&request.url)?;
    let extension = file_extension(&request, &source);
    ensure!(
        extension != "m3u8" && extension != "mpd",
        "Segmented HLS/DASH downloads are not supported yet"
    );
    let final_path = media_path(&root, &request, &extension);
    let part_path = final_path.with_extension(format!("{extension}.part"));
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut builder = client.get(source);
    for (name, value) in &request.request_headers {
        builder = builder.header(
            HeaderName::from_bytes(name.as_bytes()).context("Invalid source header name")?,
            HeaderValue::from_str(value).context("Invalid source header value")?,
        );
    }
    let mut response = builder
        .send()
        .context("Could not connect to the source")?
        .error_for_status()
        .context("The source rejected the download")?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    ensure!(
        !content_type.contains("mpegurl") && !content_type.contains("dash+xml"),
        "Segmented HLS/DASH downloads are not supported yet"
    );
    let total = response.content_length();
    {
        let mut state = manager
            .lock()
            .map_err(|_| anyhow::anyhow!("Download manager is unavailable"))?;
        if let Some(record) = state.records.iter_mut().find(|record| record.id == id) {
            record.total_bytes = total;
            record.file_path = Some(final_path.clone());
        }
    }

    let mut output = File::create(&part_path)?;
    let mut buffer = vec![0_u8; 256 * 1024];
    let mut downloaded = 0_u64;
    loop {
        let count = response.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if is_cancelled(manager, id) {
            drop(output);
            let _ = fs::remove_file(&part_path);
            bail!("Download cancelled");
        }
        output.write_all(&buffer[..count])?;
        downloaded += count as u64;
        if let Ok(mut state) = manager.lock()
            && let Some(record) = state.records.iter_mut().find(|record| record.id == id)
        {
            record.bytes_downloaded = downloaded;
        }
    }
    output.sync_all()?;
    if final_path.exists() {
        fs::remove_file(&final_path)?;
    }
    fs::rename(&part_path, &final_path)?;
    Ok(())
}

fn is_cancelled(manager: &Arc<Mutex<DownloadManager>>, id: &str) -> bool {
    manager.lock().map_or(true, |state| {
        state.cancelled.contains(id)
            || state
                .records
                .iter()
                .any(|record| record.id == id && record.status == "cancelled")
    })
}

fn cache_artwork(
    client: &Client,
    root: &Path,
    request: &DownloadRequest,
) -> Result<Option<PathBuf>> {
    let Some(value) = request
        .poster_url
        .as_deref()
        .or(request.backdrop_url.as_deref())
    else {
        return Ok(None);
    };
    let url = Url::parse(value)?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "Artwork URL is not HTTP"
    );
    let extension = image_extension(&url);
    let target = root.join(".artwork").join(format!(
        "{}.{}",
        safe_component(&request.content_id),
        extension
    ));
    if target.exists() {
        return Ok(Some(target));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = client.get(url).send()?.error_for_status()?.bytes()?;
    ensure!(
        bytes.len() <= 15 * 1024 * 1024,
        "Artwork is unexpectedly large"
    );
    fs::write(&target, bytes)?;
    Ok(Some(target))
}

fn validate_request(request: &DownloadRequest) -> Result<()> {
    ensure!(
        !request.title.trim().is_empty(),
        "Download title is required"
    );
    ensure!(
        !request.video_id.trim().is_empty(),
        "Video identity is required"
    );
    let url = Url::parse(request.url.trim()).context("The source URL is invalid")?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host().is_some(),
        "Only direct HTTP downloads are supported"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "Source URLs cannot contain embedded credentials"
    );
    crate::content::validate_addon_url(&url)?;
    ensure!(
        request.request_headers.len() <= 32,
        "A source may provide at most 32 request headers"
    );
    ensure!(
        request
            .request_headers
            .iter()
            .map(|(name, value)| name.len() + value.len())
            .sum::<usize>()
            <= 16 * 1024,
        "Source request headers are unexpectedly large"
    );
    Ok(())
}

fn to_item(record: &DownloadRecord) -> DownloadItem {
    DownloadItem {
        id: record.id.clone(),
        content_id: record.request.content_id.clone(),
        content_type: record.request.content_type.clone(),
        video_id: record.request.video_id.clone(),
        title: record.request.title.clone(),
        show_name: record.request.show_name.clone(),
        season: record.request.season,
        episode: record.request.episode,
        source_name: record.request.source_name.clone(),
        status: record.status.clone(),
        bytes_downloaded: record.bytes_downloaded,
        total_bytes: record.total_bytes,
        file_path: record
            .file_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        play_url: record
            .file_path
            .as_ref()
            .filter(|_| record.status == "completed")
            .and_then(|path| Url::from_file_path(path).ok())
            .map(|url| url.to_string()),
        artwork_cached: record
            .artwork_path
            .as_ref()
            .is_some_and(|path| path.exists()),
        error: record.error.clone(),
        created_at: record.created_at,
        skip_segments: record.skip_segments.clone(),
    }
}

fn media_path(root: &Path, request: &DownloadRequest, extension: &str) -> PathBuf {
    let show = safe_component(request.show_name.as_deref().unwrap_or(&request.title));
    if let (Some(season), Some(episode)) = (request.season, request.episode) {
        root.join(show)
            .join(format!("Season {season:02}"))
            .join(format!(
                "S{season:02}E{episode:02} - {}.{extension}",
                safe_component(&request.title)
            ))
    } else {
        root.join(show.clone()).join(format!("{show}.{extension}"))
    }
}

fn file_extension(request: &DownloadRequest, url: &Url) -> String {
    request
        .filename
        .as_deref()
        .and_then(|value| Path::new(value).extension())
        .or_else(|| Path::new(url.path()).extension())
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|value| {
            (2..=5).contains(&value.len()) && value.chars().all(|ch| ch.is_ascii_alphanumeric())
        })
        .unwrap_or_else(|| "mp4".to_string())
}

fn image_extension(url: &Url) -> &'static str {
    match Path::new(url.path())
        .extension()
        .and_then(|value| value.to_str())
    {
        Some(value) if value.eq_ignore_ascii_case("png") => "png",
        Some(value) if value.eq_ignore_ascii_case("webp") => "webp",
        _ => "jpg",
    }
}

fn safe_component(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|ch| {
            if ch.is_control() || "<>:\"/\\|?*".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        "Untitled".to_string()
    } else {
        cleaned.chars().take(100).collect()
    }
}

fn move_file(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    if target.exists() {
        fs::remove_file(target)?;
    }
    if fs::rename(source, target).is_err() {
        fs::copy(source, target)?;
        fs::remove_file(source)?;
    }
    Ok(())
}

fn app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("Nuvio")
}

fn default_download_root() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(app_data_dir)
        .join("Downloads")
        .join("Nuvio")
}

fn records_path() -> PathBuf {
    app_data_dir().join("downloads.json")
}
fn settings_path() -> PathBuf {
    app_data_dir().join("download-settings.json")
}

fn load_records() -> Result<Vec<DownloadRecord>> {
    let path = records_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn load_root() -> Option<PathBuf> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(settings_path()).ok()?).ok()?;
    value.get("root")?.as_str().map(PathBuf::from)
}

fn save_root(root: &Path) -> Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(&serde_json::json!({ "root": root }))?,
    )?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_windows_filename_characters() {
        assert_eq!(safe_component("Law & Order: SVU?"), "Law & Order_ SVU_");
        assert_eq!(safe_component("..."), "Untitled");
    }

    #[test]
    fn episode_paths_are_grouped_by_show_and_season() {
        let request = DownloadRequest {
            content_id: "tt1".into(),
            content_type: "series".into(),
            video_id: "tt1:2:3".into(),
            title: "Third / Episode".into(),
            show_name: Some("Example".into()),
            season: Some(2),
            episode: Some(3),
            poster_url: None,
            backdrop_url: None,
            url: "https://example.com/video.mkv".into(),
            request_headers: HashMap::new(),
            source_name: String::new(),
            filename: None,
        };
        assert!(
            media_path(Path::new("D:/Media"), &request, "mkv")
                .ends_with("Example/Season 02/S02E03 - Third _ Episode.mkv")
        );
    }
}

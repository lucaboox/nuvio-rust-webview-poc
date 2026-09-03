use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    thread,
};

use crate::app_state::AppState;

const SETTINGS_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProgressCheckpoint {
    position_ms: i64,
    duration_ms: i64,
    reached_eof: bool,
}

/// Keep only the freshest queued position while a previous network push is in
/// flight. EOF is sticky so even a defensive out-of-order enqueue can never
/// turn a completed playback into an ordinary resume point.
fn coalesce_progress_checkpoints(
    receiver: &Receiver<ProgressCheckpoint>,
    mut latest: ProgressCheckpoint,
) -> ProgressCheckpoint {
    while let Ok(next) = receiver.try_recv() {
        let reached_eof = latest.reached_eof || next.reached_eof;
        latest = ProgressCheckpoint {
            reached_eof,
            ..next
        };
    }
    latest
}

fn spawn_progress_reporter(
    auth: crate::auth::AuthService,
    profile_id: i32,
    identity: crate::progress::PlaybackIdentity,
) -> Box<dyn Fn(i64, i64, bool) + Send + 'static> {
    let (sender, receiver) = mpsc::channel::<ProgressCheckpoint>();
    if let Err(error) = thread::Builder::new()
        .name("nuvio-progress-sync".to_string())
        .spawn(move || {
            while let Ok(first) = receiver.recv() {
                let checkpoint = coalesce_progress_checkpoints(&receiver, first);
                if let Err(error) = crate::progress::push(
                    &auth,
                    profile_id,
                    &identity,
                    checkpoint.position_ms,
                    checkpoint.duration_ms,
                    checkpoint.reached_eof,
                ) {
                    eprintln!("watch progress push failed: {error:#}");
                }
            }
        })
    {
        eprintln!("could not start watch progress worker: {error}");
    }
    Box::new(move |position_ms, duration_ms, reached_eof| {
        // std::mpsc::Sender::send only appends to an in-memory queue; all HTTP
        // and credential work remains on the named reporter thread.
        let _ = sender.send(ProgressCheckpoint {
            position_ms,
            duration_ms,
            reached_eof,
        });
    })
}

#[derive(Debug, Deserialize)]
pub struct RequestEnvelope {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ResponseEnvelope {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Serialize)]
pub struct EventEnvelope {
    pub event: &'static str,
    pub payload: Value,
}

pub enum OutboundMessage {
    Response(ResponseEnvelope),
    Event(EventEnvelope),
}

/// Release notes are public GitHub data and do not need the global account
/// lock. Keeping this request independent also means a slow GitHub response
/// cannot hold up player or download state.
pub fn handle_updates_shared(raw: &str) -> Option<Vec<OutboundMessage>> {
    let request = serde_json::from_str::<RequestEnvelope>(raw).ok()?;
    if request.method != "updates.changelog" {
        return None;
    }
    let response = match crate::updates::github_release_notes() {
        Ok(releases) => success(request.id, json!({ "releases": releases })),
        Err(error) => failure(request.id, "changelog_load_failed", error.to_string()),
    };
    Some(vec![OutboundMessage::Response(response)])
}

/// Player controls use their own lock and bypass account/content work. This
/// keeps seek, volume, pause, and track selection responsive while an unrelated
/// Supabase or addon request is in flight.
pub fn handle_player_shared(
    raw: &str,
    shared_state: &Arc<Mutex<AppState>>,
) -> Option<Vec<OutboundMessage>> {
    let request = serde_json::from_str::<RequestEnvelope>(raw).ok()?;
    if !request.method.starts_with("player.")
        || matches!(
            request.method.as_str(),
            "player.prepare" | "player.thumbnail" | "player.skipSegments"
        )
    {
        return None;
    }
    let player = Arc::clone(&shared_state.lock().ok()?.player);
    let mut player = player.lock().ok()?;
    let response = handle_player_command(&request, &mut player)?;
    Some(vec![OutboundMessage::Response(response)])
}

/// Download state has its own lock because transfers continue while account
/// and catalog calls run. The UI polls this lightweight snapshot for progress.
pub fn handle_downloads_shared(
    raw: &str,
    shared_state: &Arc<Mutex<AppState>>,
) -> Option<Vec<OutboundMessage>> {
    let request = serde_json::from_str::<RequestEnvelope>(raw).ok()?;
    if !request.method.starts_with("downloads.") {
        return None;
    }
    let downloads = Arc::clone(&shared_state.lock().ok()?.downloads);
    let id = request.id.clone();
    let response = match request.method.as_str() {
        "downloads.list" => match downloads.lock() {
            Ok(state) => success(id, json!({ "root": state.root(), "items": state.items() })),
            Err(_) => failure(
                id,
                "downloads_unavailable",
                "Download manager is unavailable".to_string(),
            ),
        },
        "downloads.enqueue" => {
            let parsed = request.params.get("request").cloned().and_then(|value| {
                serde_json::from_value::<crate::downloads::DownloadRequest>(value).ok()
            });
            match parsed {
                Some(item) => match crate::downloads::enqueue(&downloads, item) {
                    Ok(item) => success(id, json!({ "item": item })),
                    Err(error) => failure(id, "download_rejected", error.to_string()),
                },
                None => failure(
                    id,
                    "invalid_params",
                    "Download request is required".to_string(),
                ),
            }
        }
        "downloads.cancel" => match string_param(&request.params, "id") {
            Some(item_id) => match downloads
                .lock()
                .map_err(|_| anyhow::anyhow!("Download manager is unavailable"))
                .and_then(|mut state| state.cancel(item_id))
            {
                Ok(()) => success(id, json!({ "cancelled": true })),
                Err(error) => failure(id, "download_cancel_failed", error.to_string()),
            },
            None => failure(id, "invalid_params", "Download id is required".to_string()),
        },
        "downloads.remove" => match string_param(&request.params, "id") {
            Some(item_id) => match downloads
                .lock()
                .map_err(|_| anyhow::anyhow!("Download manager is unavailable"))
                .and_then(|mut state| state.remove(item_id))
            {
                Ok(()) => success(id, json!({ "removed": true })),
                Err(error) => failure(id, "download_remove_failed", error.to_string()),
            },
            None => failure(id, "invalid_params", "Download id is required".to_string()),
        },
        "downloads.retry" => match string_param(&request.params, "id") {
            Some(item_id) => match crate::downloads::retry(&downloads, item_id) {
                Ok(()) => success(id, json!({ "queued": true })),
                Err(error) => failure(id, "download_retry_failed", error.to_string()),
            },
            None => failure(id, "invalid_params", "Download id is required".to_string()),
        },
        "downloads.moveStorage" => match string_param(&request.params, "path") {
            Some(path) => match downloads
                .lock()
                .map_err(|_| anyhow::anyhow!("Download manager is unavailable"))
                .and_then(|mut state| state.move_storage(std::path::Path::new(path)))
            {
                Ok(()) => success(id, json!({ "moved": true })),
                Err(error) => failure(id, "download_move_failed", error.to_string()),
            },
            None => failure(
                id,
                "invalid_params",
                "A destination folder is required".to_string(),
            ),
        },
        "downloads.artwork" => match string_param(&request.params, "id") {
            Some(item_id) => match downloads
                .lock()
                .map_err(|_| anyhow::anyhow!("Download manager is unavailable"))
                .and_then(|state| state.artwork(item_id))
            {
                Ok(Some((bytes, mime))) => success(
                    id,
                    json!({ "image": format!("data:{mime};base64,{}", base64(&bytes)) }),
                ),
                Ok(None) => success(id, json!({ "image": Value::Null })),
                Err(error) => failure(id, "download_artwork_failed", error.to_string()),
            },
            None => failure(id, "invalid_params", "Download id is required".to_string()),
        },
        "downloads.openFolder" => {
            let root = downloads.lock().ok().map(|state| state.root());
            match root.and_then(|path| open::that(path).ok()) {
                Some(()) => success(id, json!({ "opened": true })),
                None => failure(
                    id,
                    "download_folder_failed",
                    "Could not open the download folder".to_string(),
                ),
            }
        }
        _ => failure(
            id,
            "method_not_found",
            format!("Unknown native method: {}", request.method),
        ),
    };
    Some(vec![OutboundMessage::Response(response)])
}

fn handle_player_command(
    request: &RequestEnvelope,
    player: &mut crate::player::PlayerService,
) -> Option<ResponseEnvelope> {
    let id = request.id.clone();
    let response = match request.method.as_str() {
        "player.capabilities" => success(id, json!(player.capabilities())),
        "player.state" => success(id, json!(player.state())),
        "player.togglePause" => unit_result(id, player.toggle_pause()),
        "player.seek" => unit_result(
            id,
            player.seek(
                request
                    .params
                    .get("positionMs")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            ),
        ),
        "player.seekRelative" => unit_result(
            id,
            player.seek_relative(
                request
                    .params
                    .get("offsetMs")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            ),
        ),
        "player.setVolume" => unit_result(
            id,
            player.set_volume(
                request
                    .params
                    .get("volume")
                    .and_then(Value::as_i64)
                    .unwrap_or(100),
            ),
        ),
        "player.toggleMute" => unit_result(id, player.toggle_mute()),
        "player.cycleAudio" => unit_result(id, player.cycle_audio()),
        "player.cycleSubtitle" => unit_result(id, player.cycle_subtitle()),
        "player.setSpeed" => match request.params.get("speed").and_then(Value::as_f64) {
            Some(speed) => unit_result(id, player.set_speed(speed)),
            None => failure(id, "invalid_params", "A speed is required".to_string()),
        },
        "player.setResizeMode" => match string_param(&request.params, "mode") {
            Some(mode) => unit_result(
                id,
                player.set_resize_mode(crate::player::ResizeMode::from_setting(&mode)),
            ),
            None => failure(id, "invalid_params", "A resize mode is required".to_string()),
        },
        "player.setAudioTrack" => match request.params.get("id").and_then(Value::as_i64) {
            Some(track) => unit_result(id, player.set_audio_track(track)),
            None => failure(id, "invalid_params", "A track id is required".to_string()),
        },
        "player.setSubtitleTrack" => match request.params.get("id").and_then(Value::as_i64) {
            Some(track) => unit_result(id, player.set_subtitle_track(track)),
            None => failure(id, "invalid_params", "A track id is required".to_string()),
        },
        "player.stop" => {
            player.stop();
            success(id, json!({ "stopped": true }))
        }
        _ => return None,
    };
    Some(response)
}

/// Runs addon HTTP work behind its own lock so a slow catalog cannot block
/// profile switching, settings, window commands, or other account IPC.
pub fn handle_content_shared(
    raw: &str,
    shared_state: &Arc<Mutex<AppState>>,
) -> Option<Vec<OutboundMessage>> {
    let request = serde_json::from_str::<RequestEnvelope>(raw).ok()?;
    if request.method == "player.skipSegments" {
        let content_id = string_param(&request.params, "contentId").unwrap_or_default();
        let video_id = string_param(&request.params, "videoId").unwrap_or_default();
        let season = request
            .params
            .get("season")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let episode = request
            .params
            .get("episode")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let (cached, options) = {
            let state = shared_state.lock().ok()?;
            let cached = state
                .downloads
                .lock()
                .ok()?
                .cached_segments(content_id, video_id, season, episode);
            (cached, state.skip_options())
        };
        let segments = cached.unwrap_or_else(|| {
            crate::skip_segments::resolve_with_options(
                content_id, video_id, season, episode, &options,
            )
            .unwrap_or_default()
        });
        return Some(vec![OutboundMessage::Response(success(
            request.id,
            json!({ "segments": segments }),
        ))]);
    }
    if request.method == "player.thumbnail" {
        let position_ms = request
            .params
            .get("positionMs")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let player = Arc::clone(&shared_state.lock().ok()?.player);
        let source = player.lock().ok()?.source();
        let dll = crate::player::find_mpv();
        let response = match (source, dll) {
            (Some((url, headers)), Some(dll)) => {
                match crate::thumbnail::capture(&dll, &url, &headers, position_ms) {
                    Ok(bytes) => success(
                        request.id,
                        json!({ "image": format!("data:image/jpeg;base64,{}", base64(&bytes)) }),
                    ),
                    Err(error) => {
                        // Otherwise a broken thumbnailer is completely silent:
                        // the UI drops the preview and shows nothing.
                        eprintln!("seek thumbnail failed at {position_ms}ms: {error:#}");
                        failure(request.id, "thumbnail_failed", error.to_string())
                    }
                }
            }
            _ => failure(
                request.id,
                "thumbnail_unavailable",
                "Nothing is playing".to_string(),
            ),
        };
        return Some(vec![OutboundMessage::Response(response)]);
    }
    if !request.method.starts_with("content.") {
        return None;
    }
    let (mut content, mut addons, mut metadata_config, mut home_layout, layout_load) = {
        let state = shared_state.lock().ok()?;
        (
            Arc::clone(&state.content),
            state.addons.clone(),
            state.metadata_config.clone(),
            state.home_layout.clone(),
            if request.method == "content.home" {
                state.pending_home_layout_load()
            } else {
                None
            },
        )
    };
    if let Some(layout_load) = layout_load {
        // Manifest, collection, and organizer requests run without the global
        // AppState mutex. Profile/settings/player IPC remains responsive while
        // a slow addon or backend is loading the home page.
        let started = std::time::Instant::now();
        let plan = layout_load
            .load()
            .map(|layout| layout.plan())
            .unwrap_or_default();
        let elapsed_ms = started.elapsed().as_millis();

        // Always take a fresh content snapshot after the remote work. If the
        // captured profile/addon generation became stale, its plan is rejected
        // and this response uses the current profile's inputs instead.
        let mut state = shared_state.lock().ok()?;
        let _ = state.commit_home_layout_load(&layout_load, plan, elapsed_ms);
        content = Arc::clone(&state.content);
        addons = state.addons.clone();
        metadata_config = state.metadata_config.clone();
        home_layout = if state.home_layout_stale {
            Default::default()
        } else {
            state.home_layout.clone()
        };
    }
    let id = request.id.clone();
    if request.method == "content.enrichMeta" {
        let item =
            request.params.get("item").cloned().and_then(|value| {
                serde_json::from_value::<crate::content::ContentMeta>(value).ok()
            });
        let client = match content.lock() {
            Ok(content) => content.http_client(),
            Err(_) => {
                return Some(vec![OutboundMessage::Response(failure(
                    id,
                    "content_unavailable",
                    "The content service lock was interrupted".to_string(),
                ))]);
            }
        };
        return Some(vec![OutboundMessage::Response(match item {
            Some(item) => success(
                id,
                json!(crate::metadata::enrich_ratings(
                    &client,
                    item,
                    &metadata_config
                )),
            ),
            None => failure(
                id,
                "invalid_params",
                "Metadata enrichment requires an item".to_string(),
            ),
        })]);
    }
    if request.method == "content.personDetails" {
        let person_id = request.params.get("personId").and_then(Value::as_i64);
        let prefer_crew = request.params.get("preferCrew").and_then(Value::as_bool);
        let client = match content.lock() {
            Ok(content) => content.http_client(),
            Err(_) => {
                return Some(vec![OutboundMessage::Response(failure(
                    id,
                    "content_unavailable",
                    "The content service lock was interrupted".to_string(),
                ))]);
            }
        };
        return Some(vec![OutboundMessage::Response(match person_id {
            Some(person_id) if person_id > 0 => match crate::metadata::fetch_person_detail(
                &client,
                person_id,
                prefer_crew,
                &metadata_config.tmdb,
            ) {
                Ok(person) => success(id, json!(person)),
                Err(error) => failure(id, "person_details_failed", error.to_string()),
            },
            _ => failure(
                id,
                "invalid_params",
                "Person details require a positive TMDB personId".to_string(),
            ),
        })]);
    }
    let mut content = match content.lock() {
        Ok(mut content) => content.snapshot(&addons),
        Err(_) => {
            return Some(vec![OutboundMessage::Response(failure(
                id,
                "content_unavailable",
                "The content service lock was interrupted".to_string(),
            ))]);
        }
    };
    let response = match request.method.as_str() {
        "content.home" => success(id, json!(content.load_home(&addons, &home_layout))),
        "content.discoverCatalogs" => success(
            id,
            json!({ "catalogs": content.discover_catalogs(&addons) }),
        ),
        "content.search" => match string_param(&request.params, "query") {
            Some(query) => success(id, json!(content.search(&addons, query))),
            None => failure(id, "invalid_params", "Search requires a query".to_string()),
        },
        "content.catalog" => {
            let manifest_url = string_param(&request.params, "manifestUrl");
            let content_type = string_param(&request.params, "type");
            let catalog_id = string_param(&request.params, "catalogId");
            let genre = string_param(&request.params, "genre");
            let skip = request
                .params
                .get("skip")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(0);
            match (manifest_url, content_type, catalog_id) {
                (Some(manifest_url), Some(content_type), Some(catalog_id)) => {
                    match content.catalog(
                        &addons,
                        manifest_url,
                        content_type,
                        catalog_id,
                        genre,
                        skip,
                    ) {
                        Ok(section) => success(id, json!(section)),
                        Err(error) => failure(id, "catalog_failed", error.to_string()),
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Catalog requires manifestUrl, type, and catalogId".to_string(),
                ),
            }
        }
        "content.details" => {
            let content_type = string_param(&request.params, "type");
            let content_id = string_param(&request.params, "id");
            match (content_type, content_id) {
                (Some(content_type), Some(content_id)) => {
                    match content.resolve_meta_canonical(
                        &addons,
                        content_type,
                        content_id,
                        &metadata_config,
                    ) {
                        Ok(details) => success(id, json!(details)),
                        Err(error) => failure(id, "details_failed", error.to_string()),
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Details require type and id".to_string(),
                ),
            }
        }
        "content.resolveMeta" => {
            let content_type = string_param(&request.params, "type");
            let content_id = string_param(&request.params, "id");
            match (content_type, content_id) {
                (Some(content_type), Some(content_id)) => {
                    match content.resolve_meta(&addons, content_type, content_id) {
                        Ok(details) => success(id, json!(details)),
                        Err(error) => failure(id, "details_failed", error.to_string()),
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Metadata resolution requires type and id".to_string(),
                ),
            }
        }
        "content.streams" => {
            let content_type = string_param(&request.params, "type");
            let content_id = string_param(&request.params, "id");
            match (content_type, content_id) {
                (Some(content_type), Some(content_id)) => {
                    match content.streams(&addons, content_type, content_id) {
                        Ok(streams) => success(id, json!({ "streams": streams })),
                        Err(error) => failure(id, "streams_failed", error.to_string()),
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Streams require type and id".to_string(),
                ),
            }
        }
        "content.collectionFolder" => {
            let sources = request.params.get("sources").cloned().and_then(|value| {
                serde_json::from_value::<Vec<crate::collections::CollectionCatalogSource>>(value)
                    .ok()
            });
            match sources {
                Some(sources) => {
                    let (sections, errors) = content.collection_folder(&addons, &sources);
                    success(id, json!({ "sections": sections, "errors": errors }))
                }
                None => failure(
                    id,
                    "invalid_params",
                    "Collection folder sources are required".to_string(),
                ),
            }
        }
        _ => failure(
            id,
            "method_not_found",
            format!("Unknown native method: {}", request.method),
        ),
    };
    Some(vec![OutboundMessage::Response(response)])
}

pub fn handle(raw: &str, state: &mut AppState) -> Vec<OutboundMessage> {
    let request = match serde_json::from_str::<RequestEnvelope>(raw) {
        Ok(request) => request,
        Err(error) => {
            return vec![OutboundMessage::Response(failure(
                "unknown".to_string(),
                "invalid_request",
                format!("Invalid bridge request: {error}"),
            ))];
        }
    };

    let id = request.id.clone();
    if request.method == "app.bootstrap" {
        let _ = state.restore_saved_account();
    }
    let response = match request.method.as_str() {
        "app.bootstrap" => success(
            id,
            json!({
                "appName": "Nuvio",
                "architecture": "Rust + Wry + React",
                "platform": std::env::consts::OS,
                "protocolVersion": 2,
                "player": state.player.lock().map(|player| player.capabilities()).ok(),
                "auth": state.auth.snapshot(),
                "profiles": state.profiles,
                "activeProfileIndex": state.active_profile_index,
                "addons": state.addons,
                "settings": state.settings_snapshot,
                "uptimeMs": state.started_at.elapsed().as_millis(),
                // Where the wait went, so a slow launch can be attributed to a
                // step rather than guessed at.
                "bootTimings": state
                    .boot_timings
                    .iter()
                    .map(|(label, ms)| json!({ "step": label, "ms": ms }))
                    .collect::<Vec<_>>(),
            }),
        ),
        // The shared UI's `platform.request`. Stateless, so it sits with the
        // other calls that need nothing but their parameters.
        "http.request" => match serde_json::from_value::<crate::http::HttpRequest>(
            request.params.clone(),
        ) {
            Ok(input) => match crate::http::request(input) {
                Ok(response) => success(id, json!(response)),
                // A host that refused is reported through the response, not
                // here: only a request that never completed is an error.
                Err(error) => failure(id, "request_failed", error.to_string()),
            },
            Err(error) => failure(id, "request_invalid", error.to_string()),
        },
        "ui.ping" => {
            state.ping_count += 1;
            success(
                id,
                json!({
                    "message": "Rust replied",
                    "roundTrip": state.ping_count,
                    "echo": request.params,
                }),
            )
        }
        "auth.state" => success(id, account_payload(state, None)),
        // The shared UI's `auth.request`: it names a path, the shell signs and
        // sends it. No token crosses back.
        "auth.request" => {
            let path = string_param(&request.params, "path").unwrap_or_default().to_string();
            let init = request.params.get("init").cloned().unwrap_or_else(|| json!({}));
            let method = init
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET")
                .to_string();
            let body = init.get("body").and_then(Value::as_str).map(str::to_string);
            let headers = init
                .get("headers")
                .and_then(Value::as_object)
                .map(|map| {
                    map.iter()
                        .filter_map(|(name, value)| {
                            value.as_str().map(|text| (name.clone(), text.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            match state.auth.authorized_request(&path, &method, body, headers) {
                Ok(value) => success(id, value),
                Err(error) => failure(id, "account_request_failed", error.to_string()),
            }
        }
        "auth.configureBackend" => {
            let self_hosted = request.params.get("selfHosted").and_then(Value::as_bool);
            match self_hosted {
                Some(self_hosted) => match state.auth.configure_backend(
                    self_hosted,
                    string_param(&request.params, "backendUrl"),
                    string_param(&request.params, "publishableKey"),
                ) {
                    Ok(snapshot) => {
                        state.profiles.clear();
                        state.addons.clear();
                        state.settings_snapshot = None;
                        state.settings_blob = None;
                        state.provider_credentials = None;
                        state.metadata_config = Default::default();
                        state.content.lock().unwrap().invalidate();
                        state.set_active_profile_index(1);
                        state.invalidate_home_layout();
                        state.session_restore_attempted = true;
                        success(id, json!({ "auth": snapshot }))
                    }
                    Err(error) => failure(id, "backend_configuration_failed", error.to_string()),
                },
                None => failure(
                    id,
                    "invalid_params",
                    "Choose whether to use a self-hosted backend".to_string(),
                ),
            }
        }
        "auth.continueAnonymous" => {
            state.auth.continue_anonymously();
            let warning = state
                .refresh_account_data()
                .err()
                .map(|error| error.to_string());
            success(id, account_payload(state, warning))
        }
        // `auth.state` reports a session; it does not restore one. The old UI
        // got restoration as a side effect of `app.bootstrap` at startup, which
        // the shared UI never calls — so a relaunch reached for profiles with
        // no session behind them and was told to sign in first.
        "auth.restore" => match state.auth.restore_session() {
            Ok(true) => {
                let warning = state
                    .refresh_account_data()
                    .err()
                    .map(|error| error.to_string());
                success(id, account_payload(state, warning))
            }
            Ok(false) => failure(
                id,
                "no_session",
                "No stored session to restore".to_string(),
            ),
            Err(error) => failure(id, "auth_failed", error.to_string()),
        },
        "auth.signIn" => {
            let credentials = credentials(&request.params);
            match credentials.and_then(|(email, password)| state.auth.sign_in(email, password)) {
                Ok(_) => {
                    let warning = state
                        .refresh_account_data()
                        .err()
                        .map(|error| error.to_string());
                    success(id, account_payload(state, warning))
                }
                Err(error) => failure(id, "auth_failed", error.to_string()),
            }
        }
        "auth.signUp" => {
            let credentials = credentials(&request.params);
            match credentials.and_then(|(email, password)| state.auth.sign_up(email, password)) {
                Ok((_, confirmation_required)) => {
                    let warning = if confirmation_required {
                        Some("Check your email to confirm the account, then sign in.".to_string())
                    } else {
                        state
                            .refresh_account_data()
                            .err()
                            .map(|error| error.to_string())
                    };
                    success(id, account_payload(state, warning))
                }
                Err(error) => failure(id, "auth_failed", error.to_string()),
            }
        }
        "auth.signOut" => {
            state.auth.sign_out();
            state.profiles.clear();
            state.addons.clear();
            state.settings_snapshot = None;
            state.settings_blob = None;
            state.provider_credentials = None;
            state.metadata_config = Default::default();
            state.content.lock().unwrap().invalidate();
            state.set_active_profile_index(1);
            state.invalidate_home_layout();
            success(id, account_payload(state, None))
        }
        "profiles.select" => {
            let profile_index = request
                .params
                .get("profileIndex")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok());
            match profile_index.filter(|profile_index| {
                state
                    .profiles
                    .iter()
                    .any(|profile| profile.profile_index == *profile_index)
            }) {
                Some(profile_index) => {
                    state.set_active_profile_index(profile_index);
                    match state.refresh_addons() {
                        Ok(()) => {
                            if let Err(error) = state.refresh_settings() {
                                eprintln!("profile settings could not be loaded: {error:#}");
                            }
                            success(id, account_payload(state, None))
                        }
                        Err(error) => failure(id, "profile_load_failed", error.to_string()),
                    }
                }
                None => failure(
                    id,
                    "invalid_params",
                    "Unknown profile selection".to_string(),
                ),
            }
        }
        "profiles.avatars" => match state.auth.avatar_catalog() {
            Ok(avatars) => success(id, json!({ "avatars": avatars })),
            Err(error) => failure(id, "avatar_load_failed", error.to_string()),
        },
        "profiles.create" => {
            let name = string_param(&request.params, "name").map(str::to_string);
            let avatar_id = string_param(&request.params, "avatarId").map(str::to_string);
            let avatar_url = string_param(&request.params, "avatarUrl").map(str::to_string);
            let uses_primary_addons = request
                .params
                .get("usesPrimaryAddons")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match name.filter(|name| name.chars().count() <= 40) {
                Some(name) => {
                    let avatar_color = match state.auth.avatar_catalog() {
                        Ok(avatars) => avatar_id
                            .as_ref()
                            .and_then(|selected| {
                                avatars.iter().find(|avatar| &avatar.id == selected)
                            })
                            .and_then(|avatar| avatar.bg_color.clone())
                            .unwrap_or_else(|| "#6B7280".to_string()),
                        Err(_) => "#6B7280".to_string(),
                    };
                    let valid_url = avatar_url.filter(|url| {
                        url.len() <= 2048
                            && !url.chars().any(char::is_whitespace)
                            && (url.starts_with("https://") || url.starts_with("http://"))
                    });
                    match state.create_profile(
                        name,
                        avatar_color,
                        avatar_id,
                        valid_url,
                        uses_primary_addons,
                    ) {
                        Ok(()) => success(id, account_payload(state, None)),
                        Err(error) => failure(id, "profile_create_failed", error.to_string()),
                    }
                }
                None => failure(
                    id,
                    "invalid_params",
                    "Profile name is required and must be 40 characters or fewer".to_string(),
                ),
            }
        }
        "addons.refresh" => match state.refresh_addons() {
            Ok(()) => success(id, account_payload(state, None)),
            Err(error) => failure(id, "addon_load_failed", error.to_string()),
        },
        "addons.describe" => {
            let addons = state.addons.clone();
            success(
                id,
                json!({ "addons": state.content.lock().unwrap().addon_descriptors(&addons), "canEdit": state.can_edit_addons() }),
            )
        }
        "addons.refreshOne" => match string_param(&request.params, "url") {
            Some(url) => match state.addons.iter().find(|addon| addon.url == url).cloned() {
                Some(addon) => match state.content.lock().unwrap().refresh_addon(&addon) {
                    Ok(descriptor) => success(id, json!(descriptor)),
                    Err(error) => failure(id, "addon_refresh_failed", error.to_string()),
                },
                None => failure(
                    id,
                    "addon_not_found",
                    "That addon is not installed".to_string(),
                ),
            },
            None => failure(id, "invalid_params", "Addon URL is required".to_string()),
        },
        "addons.add" => match string_param(&request.params, "url") {
            Some(url) => {
                let inspection = state.content.lock().unwrap().inspect_addon(url);
                match inspection {
                    Ok((url, _name)) if state.addons.iter().any(|addon| addon.url == url) => {
                        failure(
                            id,
                            "addon_exists",
                            "That addon is already installed".to_string(),
                        )
                    }
                    Ok((url, name)) => {
                        let mut addons = state.addons.clone();
                        addons.push(crate::auth::AddonRow {
                            url,
                            name: Some(name),
                            enabled: true,
                            sort_order: addons.len() as i32,
                        });
                        match state.push_addons(addons) {
                            Ok(()) => success(id, account_payload(state, None)),
                            Err(error) => failure(id, "addon_sync_failed", error.to_string()),
                        }
                    }
                    Err(error) => failure(id, "invalid_addon", error.to_string()),
                }
            }
            None => failure(id, "invalid_params", "Enter an addon URL".to_string()),
        },
        "addons.remove" => match string_param(&request.params, "url") {
            Some(url) => {
                let addons: Vec<_> = state
                    .addons
                    .iter()
                    .filter(|addon| addon.url != url)
                    .cloned()
                    .collect();
                if addons.len() == state.addons.len() {
                    failure(
                        id,
                        "addon_not_found",
                        "That addon is not installed".to_string(),
                    )
                } else {
                    match state.push_addons(addons) {
                        Ok(()) => success(id, account_payload(state, None)),
                        Err(error) => failure(id, "addon_sync_failed", error.to_string()),
                    }
                }
            }
            None => failure(id, "invalid_params", "Addon URL is required".to_string()),
        },
        "addons.toggle" => {
            let url = string_param(&request.params, "url");
            let enabled = request.params.get("enabled").and_then(Value::as_bool);
            match (url, enabled) {
                (Some(url), Some(enabled)) => {
                    let mut found = false;
                    let addons = state
                        .addons
                        .iter()
                        .cloned()
                        .map(|mut addon| {
                            if addon.url == url {
                                addon.enabled = enabled;
                                found = true;
                            }
                            addon
                        })
                        .collect();
                    if !found {
                        failure(
                            id,
                            "addon_not_found",
                            "That addon is not installed".to_string(),
                        )
                    } else {
                        match state.push_addons(addons) {
                            Ok(()) => success(id, account_payload(state, None)),
                            Err(error) => failure(id, "addon_sync_failed", error.to_string()),
                        }
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Addon URL and enabled state are required".to_string(),
                ),
            }
        }
        "addons.move" => {
            let from = request
                .params
                .get("from")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok());
            let to = request
                .params
                .get("to")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok());
            match (from, to) {
                (Some(from), Some(to)) if from < state.addons.len() && to < state.addons.len() => {
                    let mut addons = state.addons.clone();
                    let addon = addons.remove(from);
                    addons.insert(to, addon);
                    match state.push_addons(addons) {
                        Ok(()) => success(id, account_payload(state, None)),
                        Err(error) => failure(id, "addon_sync_failed", error.to_string()),
                    }
                }
                _ => failure(id, "invalid_params", "Invalid addon order".to_string()),
            }
        }
        "settings.load" => {
            // A cached snapshot cannot see a change made on another device, so
            // it expires. Nuvio re-pulls on a 4-minute timer; opening the page
            // is a better moment for a desktop client, and the cache still
            // covers repeated visits.
            let stale = state
                .settings_loaded_at
                .map(|at| at.elapsed() >= SETTINGS_MAX_AGE)
                .unwrap_or(true);
            if (state.settings_snapshot.is_none() || stale)
                && let Err(error) = state.refresh_settings()
                && state.settings_snapshot.is_none()
            {
                // Only fail when there is nothing to show; a refresh that could
                // not reach the network should still serve what we already had.
                failure(id, "settings_load_failed", error.to_string())
            } else {
                success(id, json!(state.settings_snapshot))
            }
        }
        "settings.update" => {
            let profile_index = profile_index_param(&request.params);
            let key = string_param(&request.params, "key");
            let value = request.params.get("value").cloned();
            match (profile_index, key, value) {
                (Some(profile_index), _, _) if profile_index != state.active_profile_index => {
                    failure(
                        id,
                        "profile_changed",
                        "The active profile changed before this setting could be saved".to_string(),
                    )
                }
                (Some(_), Some(key), Some(value)) => {
                    let stale = state
                        .settings_loaded_at
                        .map(|at| at.elapsed() >= SETTINGS_MAX_AGE)
                        .unwrap_or(true);
                    if (state.settings_blob.is_none() || stale)
                        && let Err(error) = state.refresh_settings()
                    {
                        failure(id, "settings_update_failed", error.to_string())
                    } else if matches!(
                        (key, value.as_bool()),
                        ("tmdbEnabled" | "mdbListEnabled", Some(true))
                    ) && !state
                        .provider_credentials
                        .as_ref()
                        .is_some_and(|credentials| {
                            credentials.has_api_key(if key == "tmdbEnabled" {
                                "tmdb"
                            } else {
                                "mdblist"
                            })
                        })
                    {
                        failure(
                            id,
                            "settings_update_failed",
                            "Save an API key before enabling this integration".to_string(),
                        )
                    } else if key == "debridEnabled"
                        && value.as_bool() == Some(true)
                        && !state
                            .provider_credentials
                            .as_ref()
                            .is_some_and(|credentials| {
                                !credentials
                                    .configured_debrid_resolver_provider_ids()
                                    .is_empty()
                            })
                    {
                        failure(
                            id,
                            "settings_update_failed",
                            "Connect Torbox or Premiumize before enabling Debrid".to_string(),
                        )
                    } else {
                        // Official Nuvio does not persist an unavailable preferred
                        // resolver: it falls back to the first connected provider
                        // in registry order (Torbox, then Premiumize).
                        let value = if key == "debridPreferredResolverProviderId" {
                            let requested = value.as_str().unwrap_or_default();
                            let configured = state
                                .provider_credentials
                                .as_ref()
                                .map(|credentials| {
                                    credentials.configured_debrid_resolver_provider_ids()
                                })
                                .unwrap_or_default();
                            json!(
                                configured
                                    .iter()
                                    .copied()
                                    .find(|provider| *provider == requested)
                                    .or_else(|| configured.first().copied())
                                    .unwrap_or_default()
                            )
                        } else {
                            value
                        };
                        let blob = state.settings_blob.clone().unwrap_or_else(|| json!({}));
                        match crate::settings::update_cached(
                            &state.auth,
                            state.active_profile_index,
                            &blob,
                            key,
                            value,
                        ) {
                            Ok((settings, blob)) => {
                                state.settings_snapshot = Some(settings.clone());
                                state.settings_blob = Some(blob);
                                state.settings_loaded_at = Some(std::time::Instant::now());
                                state.refresh_metadata();
                                if key.starts_with("tmdb") || key.starts_with("mdbList") {
                                    // Metadata enrichment is cached by content ID. A module
                                    // change must not keep serving the pre-change response.
                                    state.content.lock().unwrap().invalidate();
                                }
                                success(id, json!(settings))
                            }
                            Err(error) => failure(id, "settings_update_failed", error.to_string()),
                        }
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Setting key and value are required".to_string(),
                ),
            }
        }
        "integrations.credentials" => {
            if profile_index_param(&request.params) != Some(state.active_profile_index) {
                failure(
                    id,
                    "profile_changed",
                    "The active profile changed before credentials could be loaded".to_string(),
                )
            } else {
                if state.provider_credentials.is_none() {
                    match crate::settings::load_provider_credentials(
                        &state.auth,
                        state.active_profile_index,
                    ) {
                        Ok(credentials) => {
                            state.provider_credentials = Some(credentials);
                            state.refresh_metadata();
                            // The settings blob may already enable a provider. If the
                            // credential retry succeeds after content was fetched without it,
                            // force that content to be enriched on its next request.
                            state.content.lock().unwrap().invalidate();
                        }
                        Err(error) => {
                            return vec![OutboundMessage::Response(failure(
                                id,
                                "integration_credentials_load_failed",
                                error.to_string(),
                            ))];
                        }
                    }
                }
                success(
                    id,
                    json!(
                        state
                            .provider_credentials
                            .as_ref()
                            .map(crate::settings::ProviderCredentialStore::snapshot)
                    ),
                )
            }
        }
        "integrations.updateCredential" => {
            let profile_index = profile_index_param(&request.params);
            let provider = string_param(&request.params, "provider");
            // Empty is meaningful here: it clears the credential. Do not use
            // `string_param`, which deliberately rejects empty strings.
            let credential = request
                .params
                .get("value")
                .or_else(|| request.params.get("apiKey"))
                .and_then(Value::as_str);
            match (profile_index, provider, credential) {
                (Some(profile_index), _, _) if profile_index != state.active_profile_index => {
                    failure(
                        id,
                        "profile_changed",
                        "The active profile changed before this credential could be saved"
                            .to_string(),
                    )
                }
                (
                    Some(_),
                    Some(
                        provider @ ("tmdb" | "mdblist" | "animeskip" | "introdb" | "debrid:torbox"
                        | "debrid:premiumize" | "debrid:realdebrid"),
                    ),
                    Some(credential),
                ) => {
                    let stale = state
                        .settings_loaded_at
                        .map(|at| at.elapsed() >= SETTINGS_MAX_AGE)
                        .unwrap_or(true);
                    if (state.settings_blob.is_none() || stale)
                        && let Err(error) = state.refresh_settings()
                    {
                        return vec![OutboundMessage::Response(failure(
                            id,
                            "integration_credential_update_failed",
                            error.to_string(),
                        ))];
                    }
                    let mut settings_error = None;
                    if credential.trim().is_empty() && matches!(provider, "tmdb" | "mdblist") {
                        let enabled_key = if provider == "tmdb" {
                            "tmdbEnabled"
                        } else {
                            "mdbListEnabled"
                        };
                        let was_enabled =
                            state.settings_snapshot.as_ref().is_some_and(|settings| {
                                if provider == "tmdb" {
                                    settings.tmdb_enabled
                                } else {
                                    settings.mdb_list_enabled
                                }
                            });
                        if was_enabled {
                            let blob = state.settings_blob.clone().unwrap_or_else(|| json!({}));
                            match crate::settings::update_cached(
                                &state.auth,
                                state.active_profile_index,
                                &blob,
                                enabled_key,
                                json!(false),
                            ) {
                                Ok((settings, blob)) => {
                                    state.settings_snapshot = Some(settings);
                                    state.settings_blob = Some(blob);
                                    state.settings_loaded_at = Some(std::time::Instant::now());
                                    state.refresh_metadata();
                                    state.content.lock().unwrap().invalidate();
                                }
                                Err(error) => settings_error = Some(error),
                            }
                        }
                    }

                    if let Some(error) = settings_error {
                        failure(
                            id,
                            "integration_credential_update_failed",
                            error.to_string(),
                        )
                    } else {
                        match crate::settings::update_provider_credential(
                            &state.auth,
                            state.active_profile_index,
                            provider,
                            credential,
                        ) {
                            Ok(credentials) => {
                                let snapshot = credentials.snapshot();
                                state.provider_credentials = Some(credentials);
                                if provider.starts_with("debrid:") {
                                    let blob =
                                        state.settings_blob.clone().unwrap_or_else(|| json!({}));
                                    let credentials = state.provider_credentials.as_ref().unwrap();
                                    match crate::settings::normalize_debrid_settings_for_credentials(
                                        &state.auth,
                                        state.active_profile_index,
                                        &blob,
                                        credentials,
                                    ) {
                                        Ok((settings, blob)) => {
                                            state.settings_snapshot = Some(settings);
                                            state.settings_blob = Some(blob);
                                            state.settings_loaded_at =
                                                Some(std::time::Instant::now());
                                        }
                                        Err(error) => {
                                            return vec![OutboundMessage::Response(failure(
                                                id,
                                                "integration_credential_update_failed",
                                                error.to_string(),
                                            ))];
                                        }
                                    }
                                }
                                state.refresh_metadata();
                                if matches!(provider, "tmdb" | "mdblist") {
                                    state.content.lock().unwrap().invalidate();
                                }
                                success(
                                    id,
                                    json!({
                                        "credentials": snapshot,
                                        "settings": state.settings_snapshot,
                                    }),
                                )
                            }
                            Err(error) => failure(
                                id,
                                "integration_credential_update_failed",
                                error.to_string(),
                            ),
                        }
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "A supported provider and credential value are required".to_string(),
                ),
            }
        }
        "continueWatching.dismiss" => {
            let content_id = string_param(&request.params, "contentId").unwrap_or_default();
            let season = request.params.get("season").and_then(Value::as_i64);
            let episode = request.params.get("episode").and_then(Value::as_i64);
            let dismissed = request
                .params
                .get("dismissed")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if content_id.is_empty() {
                failure(id, "invalid_params", "A title is required".to_string())
            } else if state.settings_blob.is_none()
                && let Err(error) = state.refresh_settings()
            {
                failure(id, "continue_watching_dismiss_failed", error.to_string())
            } else {
                let blob = state.settings_blob.clone().unwrap_or_else(|| json!({}));
                let key = crate::settings::next_up_dismiss_key(&content_id, season, episode);
                match crate::settings::set_next_up_dismissed(
                    &state.auth,
                    state.active_profile_index,
                    &blob,
                    &key,
                    dismissed,
                ) {
                    Ok((settings, blob)) => {
                        state.settings_snapshot = Some(settings.clone());
                        state.settings_blob = Some(blob);
                        success(id, json!(settings))
                    }
                    Err(error) => {
                        failure(id, "continue_watching_dismiss_failed", error.to_string())
                    }
                }
            }
        }
        "progress.clearForContent" => {
            // Nuvio's onContinueWatchingRemove deletes progress for the whole
            // title, not one episode — clearing a single row only promotes the
            // next-most-recent one and the card stays put.
            match string_param(&request.params, "contentId") {
                Some(content_id) if !content_id.is_empty() => unit_result(
                    id,
                    crate::progress::clear_content(
                        &state.auth,
                        state.active_profile_index,
                        content_id,
                    ),
                ),
                _ => failure(id, "invalid_params", "A title is required".to_string()),
            }
        }
        "homeLayout.list" => match state.load_home_layout() {
            Ok(layout) => {
                state.replace_home_layout(layout.plan());
                success(id, json!(layout.ui_state()))
            }
            Err(error) => failure(id, "home_layout_load_failed", error.to_string()),
        },
        "homeLayout.update" => {
            let key = string_param(&request.params, "key");
            let action = string_param(&request.params, "action").unwrap_or_default();
            let flag = request.params.get("enabled").and_then(Value::as_bool);
            let mutation = match action {
                "setEnabled" => match (key, flag) {
                    (Some(key), Some(enabled)) => {
                        Some(crate::home_layout::Mutation::SetEnabled { key, enabled })
                    }
                    _ => None,
                },
                "setHeroSourceEnabled" => match (key, flag) {
                    (Some(key), Some(enabled)) => {
                        Some(crate::home_layout::Mutation::SetHeroSourceEnabled { key, enabled })
                    }
                    _ => None,
                },
                "setCustomTitle" => match (key, string_param(&request.params, "title")) {
                    (Some(key), Some(title)) => {
                        Some(crate::home_layout::Mutation::SetCustomTitle { key, title })
                    }
                    _ => None,
                },
                "setHeroEnabled" => flag.map(crate::home_layout::Mutation::SetHeroEnabled),
                "setShowCatalogType" => flag.map(crate::home_layout::Mutation::SetShowCatalogType),
                "setHideUnreleasedContent" => {
                    flag.map(crate::home_layout::Mutation::SetHideUnreleasedContent)
                }
                "move" => {
                    let from = request.params.get("from").and_then(Value::as_u64);
                    let to = request.params.get("to").and_then(Value::as_u64);
                    match (from, to) {
                        (Some(from), Some(to)) => Some(crate::home_layout::Mutation::Move {
                            from: from as usize,
                            to: to as usize,
                        }),
                        _ => None,
                    }
                }
                "reset" => Some(crate::home_layout::Mutation::Reset),
                _ => None,
            };
            match mutation {
                Some(mutation) => {
                    let definitions = state.home_catalog_definitions();
                    let collections = state.synced_collections();
                    match crate::home_layout::apply(
                        &state.auth,
                        state.active_profile_index,
                        definitions,
                        &collections,
                        mutation,
                    ) {
                        Ok(layout) => {
                            state.replace_home_layout(layout.plan());
                            success(id, json!(layout.ui_state()))
                        }
                        Err(error) => failure(id, "home_layout_update_failed", error.to_string()),
                    }
                }
                None => failure(
                    id,
                    "invalid_params",
                    "Unknown or incomplete home layout action".to_string(),
                ),
            }
        }
        "library.list" => match crate::library::list(&state.auth, state.active_profile_index) {
            Ok(items) => success(id, json!({ "items": items })),
            Err(error) => failure(id, "library_load_failed", error.to_string()),
        },
        "collections.list" => {
            match crate::collections::list(&state.auth, state.active_profile_index) {
                Ok(collections) => success(id, json!({ "collections": collections })),
                Err(error) => failure(id, "collections_load_failed", error.to_string()),
            }
        }
        "collections.availableCatalogs" => match state.content.lock() {
            Ok(mut content) => success(
                id,
                json!({ "catalogs": content.available_collection_catalogs(&state.addons) }),
            ),
            Err(_) => failure(
                id,
                "content_unavailable",
                "Content service is unavailable".to_string(),
            ),
        },
        "collections.reorder" => {
            let collection_id = string_param(&request.params, "collectionId");
            let folder_id = string_param(&request.params, "folderId");
            let direction = request
                .params
                .get("direction")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            match collection_id {
                Some(collection_id) => match crate::collections::reorder(
                    &state.auth,
                    state.active_profile_index,
                    collection_id,
                    folder_id,
                    direction,
                ) {
                    Ok(collections) => success(id, json!({ "collections": collections })),
                    Err(error) => failure(id, "collections_reorder_failed", error.to_string()),
                },
                None => failure(
                    id,
                    "invalid_params",
                    "A collection id is required".to_string(),
                ),
            }
        }
        "collections.toggleCatalog" => {
            let collection_id = string_param(&request.params, "collectionId");
            let folder_id = string_param(&request.params, "folderId");
            let source = request.params.get("source").cloned().and_then(|value| {
                serde_json::from_value::<crate::collections::CollectionCatalogSource>(value).ok()
            });
            match (collection_id, folder_id, source) {
                (Some(collection_id), Some(folder_id), Some(source)) => {
                    match crate::collections::toggle_catalog(
                        &state.auth,
                        state.active_profile_index,
                        collection_id,
                        folder_id,
                        source,
                    ) {
                        Ok(collections) => success(id, json!({ "collections": collections })),
                        Err(error) => {
                            failure(id, "collection_catalog_update_failed", error.to_string())
                        }
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Collection, folder and catalog source are required".to_string(),
                ),
            }
        }
        "collections.reorderCatalog" => {
            let collection_id = string_param(&request.params, "collectionId");
            let folder_id = string_param(&request.params, "folderId");
            let source_index = request.params.get("sourceIndex").and_then(Value::as_u64);
            let direction = request
                .params
                .get("direction")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            match (collection_id, folder_id, source_index) {
                (Some(collection_id), Some(folder_id), Some(source_index)) => {
                    match crate::collections::reorder_catalog(
                        &state.auth,
                        state.active_profile_index,
                        collection_id,
                        folder_id,
                        source_index as usize,
                        direction,
                    ) {
                        Ok(collections) => success(id, json!({ "collections": collections })),
                        Err(error) => {
                            failure(id, "collection_catalog_reorder_failed", error.to_string())
                        }
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Collection, folder and catalog index are required".to_string(),
                ),
            }
        }
        "library.add" => match request
            .params
            .get("item")
            .cloned()
            .and_then(|value| serde_json::from_value::<crate::content::ContentMeta>(value).ok())
        {
            Some(item) => match crate::library::add(&state.auth, state.active_profile_index, &item)
            {
                Ok(()) => success(id, json!({ "saved": true })),
                Err(error) => failure(id, "library_add_failed", error.to_string()),
            },
            None => failure(
                id,
                "invalid_params",
                "A valid library item is required".to_string(),
            ),
        },
        "library.remove" => {
            let content_type = string_param(&request.params, "type");
            let content_id = string_param(&request.params, "id");
            match (content_type, content_id) {
                (Some(content_type), Some(content_id)) => match crate::library::remove(
                    &state.auth,
                    state.active_profile_index,
                    content_type,
                    content_id,
                ) {
                    Ok(()) => success(id, json!({ "saved": false })),
                    Err(error) => failure(id, "library_remove_failed", error.to_string()),
                },
                _ => failure(
                    id,
                    "invalid_params",
                    "Library removal requires type and id".to_string(),
                ),
            }
        }
        "progress.resume" => match string_param(&request.params, "id") {
            Some(content_id) => {
                match crate::progress::resume(&state.auth, state.active_profile_index, content_id) {
                    Ok(resume) => success(id, json!({ "resume": resume })),
                    Err(error) => failure(id, "progress_load_failed", error.to_string()),
                }
            }
            None => failure(
                id,
                "invalid_params",
                "Progress lookup requires an id".to_string(),
            ),
        },
        "progress.setWatched" => {
            let identity = request.params.get("identity").cloned().and_then(|value| {
                serde_json::from_value::<crate::progress::PlaybackIdentity>(value).ok()
            });
            let watched = request.params.get("watched").and_then(Value::as_bool);
            let title = string_param(&request.params, "title").unwrap_or_default();
            match (identity, watched) {
                (Some(identity), Some(watched)) => unit_result(
                    id,
                    crate::progress::set_watched(
                        &state.auth,
                        state.active_profile_index,
                        &identity,
                        title,
                        watched,
                    ),
                ),
                _ => failure(
                    id,
                    "invalid_params",
                    "A playback identity and watched flag are required".to_string(),
                ),
            }
        }
        "progress.clear" => {
            let identity = request.params.get("identity").cloned().and_then(|value| {
                serde_json::from_value::<crate::progress::PlaybackIdentity>(value).ok()
            });
            match identity {
                Some(identity) => unit_result(
                    id,
                    crate::progress::clear_progress(
                        &state.auth,
                        state.active_profile_index,
                        &identity,
                    ),
                ),
                None => failure(
                    id,
                    "invalid_params",
                    "A playback identity is required".to_string(),
                ),
            }
        }
        "progress.snapshot" => {
            // Snapshot once, then deltas — see watch_sync. Falls back to a full
            // pull whenever the cached cursor cannot be trusted.
            match crate::watch_sync::load(&state.auth, state.active_profile_index) {
                Ok((entries, watched_items)) => success(
                    id,
                    json!({ "entries": entries, "watchedItems": watched_items }),
                ),
                Err(error) => failure(id, "progress_load_failed", error.to_string()),
            }
        }
        "system.openExternal" => match string_param(&request.params, "url")
            .and_then(|raw| url::Url::parse(raw).ok())
            .filter(|url| matches!(url.scheme(), "http" | "https"))
        {
            Some(url) => match open::that_detached(url.as_str()) {
                Ok(()) => success(id, json!({ "opened": true })),
                Err(error) => failure(id, "open_failed", error.to_string()),
            },
            None => failure(
                id,
                "invalid_params",
                "Only HTTP(S) links can be opened".to_string(),
            ),
        },
        "player.openExternal" => {
            let raw = string_param(&request.params, "url");
            let parsed = raw.and_then(|value| url::Url::parse(value).ok());
            let allowed = parsed.as_ref().is_some_and(|url| match url.scheme() {
                "http" | "https" => {
                    url.host().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && crate::content::validate_addon_url(url).is_ok()
                }
                "file" => url.to_file_path().ok().is_some_and(|path| {
                    path.is_file()
                        && state
                            .downloads
                            .lock()
                            .is_ok_and(|downloads| downloads.contains_play_url(url.as_str()))
                }),
                _ => false,
            });
            match (parsed, allowed) {
                (Some(url), true) => match open::that_detached(url.as_str()) {
                    Ok(()) => success(id, json!({ "opened": true })),
                    Err(error) => failure(id, "open_failed", error.to_string()),
                },
                _ => failure(
                    id,
                    "invalid_params",
                    "The external player URL is not a permitted stream or download".to_string(),
                ),
            }
        }
        "content.home" => {
            let addons = state.addons.clone();
            let plan = state.home_layout.clone();
            success(
                id,
                json!(state.content.lock().unwrap().load_home(&addons, &plan)),
            )
        }
        "content.search" => {
            let query = string_param(&request.params, "query");
            match query {
                Some(query) => {
                    let addons = state.addons.clone();
                    success(
                        id,
                        json!(state.content.lock().unwrap().search(&addons, query)),
                    )
                }
                None => failure(id, "invalid_params", "Search requires a query".to_string()),
            }
        }
        "content.catalog" => {
            let manifest_url = string_param(&request.params, "manifestUrl");
            let content_type = string_param(&request.params, "type");
            let catalog_id = string_param(&request.params, "catalogId");
            let genre = string_param(&request.params, "genre");
            let skip = request
                .params
                .get("skip")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(0);
            match (manifest_url, content_type, catalog_id) {
                (Some(manifest_url), Some(content_type), Some(catalog_id)) => {
                    let addons = state.addons.clone();
                    match state.content.lock().unwrap().catalog(
                        &addons,
                        manifest_url,
                        content_type,
                        catalog_id,
                        genre,
                        skip,
                    ) {
                        Ok(section) => success(id, json!(section)),
                        Err(error) => failure(id, "catalog_failed", error.to_string()),
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Catalog requires manifestUrl, type, and catalogId".to_string(),
                ),
            }
        }
        "content.details" => {
            let manifest_url = string_param(&request.params, "manifestUrl");
            let content_type = string_param(&request.params, "type");
            let content_id = string_param(&request.params, "id");
            match (manifest_url, content_type, content_id) {
                (Some(manifest_url), Some(content_type), Some(content_id)) => {
                    let addons = state.addons.clone();
                    match state.content.lock().unwrap().details(
                        &addons,
                        manifest_url,
                        content_type,
                        content_id,
                    ) {
                        Ok(details) => success(id, json!(details)),
                        Err(error) => failure(id, "details_failed", error.to_string()),
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Details require manifestUrl, type, and id".to_string(),
                ),
            }
        }
        "content.streams" => {
            let content_type = string_param(&request.params, "type");
            let content_id = string_param(&request.params, "id");
            match (content_type, content_id) {
                (Some(content_type), Some(content_id)) => {
                    let addons = state.addons.clone();
                    match state
                        .content
                        .lock()
                        .unwrap()
                        .streams(&addons, content_type, content_id)
                    {
                        Ok(streams) => success(id, json!({ "streams": streams })),
                        Err(error) => failure(id, "streams_failed", error.to_string()),
                    }
                }
                _ => failure(
                    id,
                    "invalid_params",
                    "Streams require type and id".to_string(),
                ),
            }
        }
        "player.capabilities"
        | "player.state"
        | "player.togglePause"
        | "player.seek"
        | "player.seekRelative"
        | "player.setVolume"
        | "player.toggleMute"
        | "player.cycleAudio"
        | "player.cycleSubtitle"
        | "player.setSpeed"
        | "player.setResizeMode"
        | "player.setAudioTrack"
        | "player.setSubtitleTrack"
        | "player.stop" => state
            .player
            .lock()
            .ok()
            .and_then(|mut player| handle_player_command(&request, &mut player))
            .unwrap_or_else(|| {
                failure(
                    id,
                    "player_unavailable",
                    "Player lock was interrupted".to_string(),
                )
            }),
        "player.prepare" => {
            let media_id = request
                .params
                .get("mediaId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let url = request
                .params
                .get("url")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    request
                        .params
                        .get("externalUrl")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            let start_position_ms = request
                .params
                .get("startPositionMs")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let request_headers = player_request_headers(&request.params);
            let local_url_allowed = url.as_deref().is_none_or(|value| {
                !value.starts_with("file:")
                    || state
                        .downloads
                        .lock()
                        .is_ok_and(|downloads| downloads.contains_play_url(value))
            });
            let identity = request.params.get("progress").cloned().and_then(|value| {
                serde_json::from_value::<crate::progress::PlaybackIdentity>(value).ok()
            });
            match (media_id, identity, request_headers, local_url_allowed) {
                (Some(media_id), Some(identity), Ok(request_headers), true) => {
                    // Captured at prepare time so progress lands on the profile that
                    // started playback, even if the user switches profiles mid-episode.
                    let auth = state.auth.clone();
                    let profile_id = state.active_profile_index;
                    // Player settings are already cached during account/profile
                    // bootstrap. Applying that snapshot avoids a network read on
                    // every stream start and keeps RTX VSR in sync with the UI.
                    let player_settings = state.settings_snapshot.clone().or_else(|| {
                        crate::settings::load(&state.auth, state.active_profile_index)
                            .ok()
                            .map(|(snapshot, _)| snapshot)
                    });
                    let subtitle_style = player_settings
                        .as_ref()
                        .map(|snapshot| crate::player::SubtitleStyle {
                            font_size: snapshot.subtitle_font_size,
                            bold: snapshot.subtitle_bold,
                            text_color: snapshot.subtitle_text_color.clone(),
                            background_color: snapshot.subtitle_background_color.clone(),
                            outline_enabled: snapshot.subtitle_outline,
                            outline_color: snapshot.subtitle_outline_color.clone(),
                            outline_width: snapshot.subtitle_outline_width,
                            bottom_offset: snapshot.subtitle_bottom_offset,
                            use_libass: snapshot.use_libass,
                        })
                        .unwrap_or_default();
                    // Preferred first, secondary next — mpv reads the list
                    // left to right, which is the same order the settings
                    // present them in.
                    let languages = player_settings
                        .as_ref()
                        .map(|snapshot| {
                            // "none", "device", "default", "original" and
                            // "forced" are choices, not languages. Passing them
                            // through as codes would have mpv looking for a
                            // track tagged "none"; each is handled as a mode
                            // instead, and "device"/"default"/"original" are
                            // left to mpv's own selection because the shell has
                            // nothing better to resolve them to.
                            let codes = |first: &str, second: &str| {
                                [first, second]
                                    .into_iter()
                                    .map(|value| value.trim().to_ascii_lowercase())
                                    .filter(|value| {
                                        !matches!(
                                            value.as_str(),
                                            "" | "none"
                                                | "device"
                                                | "default"
                                                | "original"
                                                | "forced"
                                        )
                                    })
                                    .collect::<Vec<_>>()
                            };
                            let subtitle_choice = snapshot
                                .preferred_subtitle_language
                                .trim()
                                .to_ascii_lowercase();
                            crate::player::TrackLanguages {
                                audio: codes(
                                    &snapshot.preferred_audio_language,
                                    &snapshot.secondary_audio_language,
                                ),
                                subtitles: codes(
                                    &snapshot.preferred_subtitle_language,
                                    &snapshot.secondary_subtitle_language,
                                ),
                                subtitles_off: subtitle_choice == "none",
                                subtitles_forced_only: subtitle_choice == "forced",
                                subtitles_only_preferred: snapshot
                                    .subtitle_preferred_languages_only,
                                forced_with_matching_audio: snapshot.subtitle_forced_only,
                            }
                        })
                        .unwrap_or_default();
                    let rtx_super_resolution = player_settings
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.rtx_super_resolution);
                    let resize_mode = player_settings
                        .as_ref()
                        .map(|snapshot| {
                            crate::player::ResizeMode::from_setting(&snapshot.resize_mode)
                        })
                        .unwrap_or_default();
                    // Starting a title you had dismissed brings its suggestions
                    // back, the way Nuvio clears the keys when progress is
                    // recorded. Best effort: a failure here must not stop
                    // playback.
                    if let Some(blob) = state.settings_blob.clone()
                        && let Ok(Some((settings, blob))) =
                            crate::settings::clear_dismissed_for_content(
                                &state.auth,
                                profile_id,
                                &blob,
                                &identity.content_id,
                            )
                    {
                        state.settings_snapshot = Some(settings);
                        state.settings_blob = Some(blob);
                    }
                    match state
                        .player
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Player lock was interrupted"))
                        .and_then(|mut player| {
                            player.prepare(
                                media_id.to_string(),
                                url,
                                request_headers,
                                start_position_ms,
                                subtitle_style,
                                resize_mode,
                                languages,
                                rtx_super_resolution,
                                spawn_progress_reporter(auth, profile_id, identity),
                            )
                        }) {
                        Ok(status) => success(id, json!({ "status": status })),
                        Err(error) => failure(id, "playback_failed", error.to_string()),
                    }
                }
                (_, _, Err(error), _) => failure(id, "invalid_params", error.to_string()),
                (_, _, _, false) => failure(
                    id,
                    "invalid_params",
                    "Local playback is limited to completed Nuvio downloads".to_string(),
                ),
                _ => failure(
                    id,
                    "invalid_params",
                    "player.prepare requires mediaId and progress identity".to_string(),
                ),
            }
        }
        _ => failure(
            id,
            "method_not_found",
            format!("Unknown native method: {}", request.method),
        ),
    };

    let mut outbound = vec![OutboundMessage::Response(response)];
    if request.method == "player.prepare" {
        outbound.push(OutboundMessage::Event(EventEnvelope {
            event: "player.stateChanged",
            payload: json!({
                "state": "Playing",
                "detail": "direct stream opened in the embedded libmpv surface",
            }),
        }));
    }
    outbound
}

fn player_request_headers(params: &Value) -> anyhow::Result<Vec<String>> {
    let Some(headers) = params.get("requestHeaders").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    anyhow::ensure!(
        headers.len() <= 32,
        "A stream may provide at most 32 request headers"
    );
    let mut output = Vec::with_capacity(headers.len());
    let mut total_length = 0usize;
    for (name, value) in headers {
        let value = value
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Stream header values must be text"))?;
        anyhow::ensure!(
            !name.is_empty()
                && name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(
                            byte,
                            b'!' | b'#'
                                | b'$'
                                | b'%'
                                | b'&'
                                | b'\''
                                | b'*'
                                | b'+'
                                | b'-'
                                | b'.'
                                | b'^'
                                | b'_'
                                | b'`'
                                | b'|'
                                | b'~'
                        )
                }),
            "Stream contains an invalid request-header name"
        );
        anyhow::ensure!(
            !matches!(
                name.to_ascii_lowercase().as_str(),
                "host" | "content-length" | "transfer-encoding" | "connection" | "upgrade"
            ),
            "Stream cannot override the {name} request header"
        );
        anyhow::ensure!(
            !value.contains(['\r', '\n']) && value.len() <= 4096,
            "Stream contains an invalid request-header value"
        );
        total_length += name.len() + value.len() + 2;
        anyhow::ensure!(
            total_length <= 16 * 1024,
            "Stream request headers are too large"
        );
        output.push(format!("{name}: {value}"));
    }
    Ok(output)
}

fn unit_result(id: String, result: anyhow::Result<()>) -> ResponseEnvelope {
    match result {
        Ok(()) => success(id, json!({ "ok": true })),
        Err(error) => failure(id, "player_command_failed", error.to_string()),
    }
}

/// Small base64 encoder so the JPEG can ride the JSON bridge as a data URI.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let triple = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn string_param<'a>(params: &'a Value, name: &str) -> Option<&'a str> {
    params
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn profile_index_param(params: &Value) -> Option<i32> {
    params
        .get("profileIndex")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn credentials(params: &Value) -> anyhow::Result<(&str, &str)> {
    let email = params
        .get("email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| value.contains('@'))
        .ok_or_else(|| anyhow::anyhow!("Enter a valid email address"))?;
    let password = params
        .get("password")
        .and_then(Value::as_str)
        .filter(|value| value.len() >= 6)
        .ok_or_else(|| anyhow::anyhow!("Password must be at least 6 characters"))?;
    Ok((email, password))
}

fn account_payload(state: &AppState, warning: Option<String>) -> Value {
    json!({
        "auth": state.auth.snapshot(),
        "profiles": state.profiles,
        "activeProfileIndex": state.active_profile_index,
        "addons": state.addons,
        "settings": state.settings_snapshot,
        "warning": warning,
    })
}

fn success(id: String, result: Value) -> ResponseEnvelope {
    ResponseEnvelope {
        id,
        ok: true,
        result: Some(result),
        error: None,
    }
}

fn failure(id: String, code: &'static str, message: String) -> ResponseEnvelope {
    ResponseEnvelope {
        id,
        ok: false,
        result: None,
        error: Some(ErrorBody { code, message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_round_trip_is_correlated_and_counted() {
        let mut state = AppState::default();
        let messages = handle(
            r#"{"id":"42","method":"ui.ping","params":{"sentAt":1}}"#,
            &mut state,
        );
        let OutboundMessage::Response(response) = &messages[0] else {
            panic!("expected response")
        };

        assert_eq!(response.id, "42");
        assert!(response.ok);
        assert_eq!(response.result.as_ref().unwrap()["roundTrip"], 1);
    }

    #[test]
    fn unknown_methods_are_rejected() {
        let mut state = AppState::default();
        let messages = handle(
            r#"{"id":"9","method":"system.shell","params":{}}"#,
            &mut state,
        );
        let OutboundMessage::Response(response) = &messages[0] else {
            panic!("expected response")
        };

        assert!(!response.ok);
        assert_eq!(response.error.as_ref().unwrap().code, "method_not_found");
    }

    #[test]
    fn delayed_setting_write_cannot_cross_profiles() {
        let mut state = AppState::default();
        state.active_profile_index = 1;
        let messages = handle(
            r#"{"id":"settings","method":"settings.update","params":{"profileIndex":2,"key":"amoledEnabled","value":true}}"#,
            &mut state,
        );
        let OutboundMessage::Response(response) = &messages[0] else {
            panic!("expected response")
        };

        assert!(!response.ok);
        assert_eq!(response.error.as_ref().unwrap().code, "profile_changed");
    }

    #[test]
    fn player_headers_reject_transport_overrides_and_injection() {
        assert!(
            player_request_headers(&json!({ "requestHeaders": { "Host": "internal" } })).is_err()
        );
        assert!(
            player_request_headers(&json!({ "requestHeaders": { "X-Test": "ok\r\nbad: yes" } }))
                .is_err()
        );
    }

    #[test]
    fn player_headers_keep_auth_and_referer_values() {
        let headers = player_request_headers(&json!({
            "requestHeaders": {
                "Authorization": "Bearer example",
                "Referer": "https://example.com/"
            }
        }))
        .unwrap();
        assert_eq!(headers.len(), 2);
        assert!(
            headers
                .iter()
                .any(|header| header == "Authorization: Bearer example")
        );
    }

    #[test]
    fn progress_worker_coalesces_to_latest_and_keeps_eof_sticky() {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(ProgressCheckpoint {
                position_ms: 2_000,
                duration_ms: 10_000,
                reached_eof: false,
            })
            .unwrap();
        sender
            .send(ProgressCheckpoint {
                position_ms: 10_000,
                duration_ms: 10_000,
                reached_eof: true,
            })
            .unwrap();
        // This should never occur in the native player, but validates that a
        // late ordinary sample cannot erase the final EOF marker.
        sender
            .send(ProgressCheckpoint {
                position_ms: 9_500,
                duration_ms: 10_000,
                reached_eof: false,
            })
            .unwrap();

        let first = receiver.recv().unwrap();
        let latest = coalesce_progress_checkpoints(&receiver, first);
        assert_eq!(latest.position_ms, 9_500);
        assert_eq!(latest.duration_ms, 10_000);
        assert!(latest.reached_eof);
    }
}

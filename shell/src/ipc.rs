use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

use crate::app_state::AppState;

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
        let segments = crate::skip_segments::resolve(content_id, video_id, season, episode)
            .unwrap_or_default();
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
    let (content, addons, metadata_config, home_layout) = {
        let state = shared_state.lock().ok()?;
        (
            Arc::clone(&state.content),
            state.addons.clone(),
            state.metadata_config.clone(),
            state.home_layout.clone(),
        )
    };
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
                "uptimeMs": state.started_at.elapsed().as_millis(),
            }),
        ),
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
        "auth.continueAnonymous" => {
            state.auth.continue_anonymously();
            let warning = state
                .refresh_account_data()
                .err()
                .map(|error| error.to_string());
            success(id, account_payload(state, warning))
        }
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
            state.content.lock().unwrap().invalidate();
            state.active_profile_index = 1;
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
                    state.active_profile_index = profile_index;
                    match state.refresh_addons() {
                        Ok(()) => {
                            state.refresh_metadata();
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
        "settings.load" => match crate::settings::load(&state.auth, state.active_profile_index) {
            Ok((settings, _)) => success(id, json!(settings)),
            Err(error) => failure(id, "settings_load_failed", error.to_string()),
        },
        "settings.update" => {
            let key = string_param(&request.params, "key");
            let value = request.params.get("value").cloned();
            match (key, value) {
                (Some(key), Some(value)) => match crate::settings::update(
                    &state.auth,
                    state.active_profile_index,
                    key,
                    value,
                ) {
                    Ok(settings) => {
                        state.refresh_metadata();
                        success(id, json!(settings))
                    }
                    Err(error) => failure(id, "settings_update_failed", error.to_string()),
                },
                _ => failure(
                    id,
                    "invalid_params",
                    "Setting key and value are required".to_string(),
                ),
            }
        }
        "homeLayout.list" => match state.load_home_layout() {
            Ok(layout) => {
                state.home_layout = layout.plan();
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
                            state.home_layout = layout.plan();
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
            let entries = crate::progress::list(&state.auth, state.active_profile_index);
            let watched_items = crate::progress::watched(&state.auth, state.active_profile_index);
            match (entries, watched_items) {
                (Ok(entries), Ok(watched_items)) => success(
                    id,
                    json!({ "entries": entries, "watchedItems": watched_items }),
                ),
                (Err(error), _) | (_, Err(error)) => {
                    failure(id, "progress_load_failed", error.to_string())
                }
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
            let identity = request.params.get("progress").cloned().and_then(|value| {
                serde_json::from_value::<crate::progress::PlaybackIdentity>(value).ok()
            });
            match (media_id, identity, request_headers) {
                (Some(media_id), Some(identity), Ok(request_headers)) => {
                    // Captured at prepare time so progress lands on the profile that
                    // started playback, even if the user switches profiles mid-episode.
                    let auth = state.auth.clone();
                    let profile_id = state.active_profile_index;
                    // Subtitle appearance is read at load time; mpv applies
                    // these per file, not globally.
                    let subtitle_style =
                        crate::settings::load(&state.auth, state.active_profile_index)
                            .map(|(snapshot, _)| crate::player::SubtitleStyle {
                                font_size: snapshot.subtitle_font_size,
                                bold: snapshot.subtitle_bold,
                                text_color: snapshot.subtitle_text_color,
                                background_color: snapshot.subtitle_background_color,
                                outline_enabled: snapshot.subtitle_outline,
                                outline_color: snapshot.subtitle_outline_color,
                                outline_width: snapshot.subtitle_outline_width,
                                bottom_offset: snapshot.subtitle_bottom_offset,
                                use_libass: snapshot.use_libass,
                            })
                            .unwrap_or_default();
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
                                // Fires once, as the playback loop tears down. Progress mid-episode
                                // is not checkpointed, so a crash loses the session.
                                Box::new(move |position, duration, reached_eof| {
                                    if let Err(error) = crate::progress::push(
                                        &auth,
                                        profile_id,
                                        &identity,
                                        position,
                                        duration,
                                        reached_eof,
                                    ) {
                                        eprintln!("watch progress push failed: {error:#}");
                                    }
                                }),
                            )
                        }) {
                        Ok(status) => success(id, json!({ "status": status })),
                        Err(error) => failure(id, "playback_failed", error.to_string()),
                    }
                }
                (_, _, Err(error)) => failure(id, "invalid_params", error.to_string()),
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
}

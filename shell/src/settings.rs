use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::auth::AuthService;
use crate::metadata::{MdbListMetadataSettings, MetadataConfig, TmdbMetadataSettings};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub amoled_enabled: bool,
    pub continue_watching_visible: bool,
    pub continue_watching_style: String,
    pub continue_watching_up_next_from_furthest_episode: bool,
    pub continue_watching_use_episode_thumbnails: bool,
    pub continue_watching_show_unaired_next_up: bool,
    pub continue_watching_blur_next_up: bool,
    /// Next-up cards hidden from Continue Watching, by nextUpDismissKey.
    pub dismissed_next_up: Vec<String>,
    pub continue_watching_show_resume_prompt_on_launch: bool,
    pub continue_watching_sort_mode: String,
    pub show_loading_overlay: bool,
    pub show_parental_guide: bool,
    pub resize_mode: String,
    pub preferred_audio_language: String,
    pub preferred_subtitle_language: String,
    pub subtitle_bold: bool,
    pub subtitle_font_size: i64,
    pub subtitle_outline: bool,
    pub reuse_last_stream: bool,
    pub reuse_last_stream_hours: i64,
    pub autoplay_mode: String,
    pub autoplay_next_episode: bool,
    pub skip_intro: bool,
    pub rtx_super_resolution: bool,
    pub show_file_size_badges: bool,
    pub badge_placement: String,
    pub episode_release_alerts: bool,
    pub poster_width: i64,
    pub poster_corner_radius: i64,
    pub poster_hide_labels: bool,
    pub poster_landscape_catalogs: bool,
    /// Next-episode card thresholds, mirroring Nuvio's PlayerNextEpisodeRules.
    pub next_episode_threshold_mode: String,
    pub next_episode_threshold_percent: f64,
    pub next_episode_threshold_minutes: f64,
    // Subtitle appearance
    pub subtitle_text_color: String,
    pub subtitle_background_color: String,
    pub subtitle_outline_color: String,
    pub subtitle_outline_width: i64,
    pub subtitle_bottom_offset: i64,
    pub subtitle_forced_only: bool,
    pub subtitle_preferred_languages_only: bool,
    pub secondary_audio_language: String,
    pub secondary_subtitle_language: String,
    pub addon_subtitle_startup_mode: String,
    pub use_libass: bool,
    // Autoplay / next episode
    pub autoplay_source: String,
    pub autoplay_selected_addons: Vec<String>,
    pub autoplay_selected_plugins: Vec<String>,
    pub autoplay_regex: String,
    pub autoplay_timeout_seconds: i64,
    pub autoplay_prefer_binge_group: bool,
    pub autoplay_reuse_binge_group: bool,
    pub autoplay_next_episode_fallback: bool,
    // Skip segments
    pub anime_skip_enabled: bool,
    // Gestures (Nuvio syncs these; they are not client-only)
    pub hold_to_speed: bool,
    pub hold_to_speed_value: f64,
    // External player
    pub external_player_enabled: bool,
    pub external_player_id: String,
    pub external_player_forward_subtitles: bool,
    pub external_player_send_skip_segments: bool,
    // TMDB enrichment (the API key is a provider credential, never part of this
    // profile-settings snapshot).
    pub tmdb_enabled: bool,
    pub tmdb_language: String,
    pub tmdb_use_trailers: bool,
    pub tmdb_use_artwork: bool,
    pub tmdb_use_basic_info: bool,
    pub tmdb_use_details: bool,
    pub tmdb_use_release_dates: bool,
    pub tmdb_use_credits: bool,
    pub tmdb_use_productions: bool,
    pub tmdb_use_networks: bool,
    pub tmdb_use_episodes: bool,
    pub tmdb_use_season_posters: bool,
    pub tmdb_use_more_like_this: bool,
    pub tmdb_use_collections: bool,
    // MDBList ratings (same credential separation as TMDB).
    pub mdb_list_enabled: bool,
    pub mdb_list_use_imdb: bool,
    pub mdb_list_use_tmdb: bool,
    pub mdb_list_use_tomatoes: bool,
    pub mdb_list_use_metacritic: bool,
    pub mdb_list_use_trakt: bool,
    pub mdb_list_use_letterboxd: bool,
    pub mdb_list_use_audience: bool,
    pub mdb_list_use_mal: bool,
    // Debrid. Provider access tokens are deliberately kept in the separate
    // provider-credential store below; only non-secret behavior is synced in
    // the profile settings blob.
    pub debrid_enabled: bool,
    pub debrid_cloud_library_enabled: bool,
    pub debrid_preferred_resolver_provider_id: String,
    pub debrid_instant_playback_preparation_limit: i64,
    pub debrid_stream_max_results: i64,
    pub debrid_stream_sort_mode: String,
    pub debrid_stream_minimum_quality: String,
    pub debrid_stream_dolby_vision_filter: String,
    pub debrid_stream_hdr_filter: String,
    pub debrid_stream_codec_filter: String,
    pub debrid_stream_preferences: String,
    pub debrid_stream_name_template: String,
    pub debrid_stream_description_template: String,
}

/// Provider credentials currently editable by this client. This is deliberately
/// separate from `SettingsSnapshot`: it is only returned by the integrations
/// command, so bootstrap/account payloads do not repeatedly expose secrets to
/// the WebView.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationCredentialSnapshot {
    pub tmdb_api_key: String,
    pub mdb_list_api_key: String,
    pub anime_skip_client_id: String,
    pub intro_db_api_key: String,
    pub torbox_api_key: String,
    pub premiumize_api_key: String,
    pub real_debrid_api_key: String,
}

#[derive(Clone)]
struct ProviderCredentialRow {
    provider: String,
    credential_json: Map<String, Value>,
}

/// Raw provider rows are retained in memory because the push RPC replaces the
/// provider set. Keeping the full credential objects prevents a TMDB edit from
/// erasing Debrid, Anime-Skip, or fields introduced by a newer Nuvio client.
#[derive(Clone, Default)]
pub struct ProviderCredentialStore {
    rows: Vec<ProviderCredentialRow>,
}

/// Nuvio stores poster card style as a JSON *string* inside the settings blob
/// rather than as typed preference entries, so it needs its own read/write path.
const POSTER_PAYLOAD_KEY: &str = "poster_card_style_settings_payload";
const CONTINUE_WATCHING_PAYLOAD_KEY: &str = "continue_watching_settings_payload";
const DEFAULT_POSTER_WIDTH: i64 = 126;
const DEFAULT_POSTER_CORNER_RADIUS: i64 = 12;
const DEFAULT_DEBRID_STREAM_NAME_TEMPLATE: &str = "{stream.resolution::exists[\"{stream.resolution} \"||\"\"]}{service.shortName::exists[\"{service.shortName}\"||\"Cloud\"]} Instant";

/// Nuvio's `nextUpDismissKey`: `contentId|season|episode`, with -1 standing in
/// for a missing season or episode.
pub fn next_up_dismiss_key(content_id: &str, season: Option<i64>, episode: Option<i64>) -> String {
    format!(
        "{}|{}|{}",
        content_id.trim(),
        season.unwrap_or(-1),
        episode.unwrap_or(-1)
    )
}

/// The continue-watching preferences ride as a JSON *string* inside the blob,
/// the same way the poster style does — so it has to be parsed out, edited,
/// and re-encoded rather than merged as an object.
fn continue_watching_payload(blob: &Value) -> Map<String, Value> {
    blob.pointer(&format!("/features/{CONTINUE_WATCHING_PAYLOAD_KEY}"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

/// Materialises Nuvio's complete StoredContinueWatchingPreferences payload
/// while retaining fields introduced by newer clients. Kotlin writes this JSON
/// with `encodeDefaults = true`; preserving the original map first means a
/// desktop edit neither drops future fields nor the user's dismissal set.
fn normalized_continue_watching_payload(payload: &Map<String, Value>) -> Map<String, Value> {
    let read_bool = |key: &str, fallback: bool| {
        payload
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(fallback)
    };
    let read_enum = |key: &str, allowed: &[&str], fallback: &str| {
        payload
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| allowed.contains(value))
            .unwrap_or(fallback)
            .to_string()
    };
    let mut normalized = payload.clone();
    normalized.insert("isVisible".to_string(), json!(read_bool("isVisible", true)));
    normalized.insert(
        "style".to_string(),
        json!(read_enum("style", &["Card", "Wide", "Poster"], "Card")),
    );
    normalized.insert(
        "upNextFromFurthestEpisode".to_string(),
        json!(read_bool("upNextFromFurthestEpisode", true)),
    );
    normalized.insert(
        "use_episode_thumbnails_in_cw".to_string(),
        json!(read_bool("use_episode_thumbnails_in_cw", true)),
    );
    normalized.insert(
        "show_unaired_next_up".to_string(),
        json!(read_bool("show_unaired_next_up", true)),
    );
    normalized.insert(
        "blur_continue_watching_next_up".to_string(),
        json!(read_bool("blur_continue_watching_next_up", false)),
    );
    normalized.insert(
        "dismissedNextUpKeys".to_string(),
        json!(normalized_dismissed_next_up_keys(payload)),
    );
    normalized.insert(
        "showResumePromptOnLaunch".to_string(),
        json!(read_bool("showResumePromptOnLaunch", true)),
    );
    normalized.insert(
        "sort_mode".to_string(),
        json!(read_enum(
            "sort_mode",
            &["DEFAULT", "STREAMING_STYLE", "SPLIT_UPCOMING"],
            "DEFAULT",
        )),
    );
    normalized
}

fn normalized_dismissed_next_up_keys(payload: &Map<String, Value>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    payload
        .get("dismissedNextUpKeys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .filter(|key| seen.insert((*key).to_string()))
        .map(str::to_string)
        .collect()
}

fn set_continue_watching_setting(blob: &mut Value, key: &str, value: Value) -> Result<()> {
    let current = continue_watching_payload(blob);
    let mut payload = normalized_continue_watching_payload(&current);
    let storage_key = match key {
        "continueWatchingVisible" => "isVisible",
        "continueWatchingStyle" => "style",
        "continueWatchingUpNextFromFurthestEpisode" => "upNextFromFurthestEpisode",
        "continueWatchingUseEpisodeThumbnails" => "use_episode_thumbnails_in_cw",
        "continueWatchingShowUnairedNextUp" => "show_unaired_next_up",
        "continueWatchingBlurNextUp" => "blur_continue_watching_next_up",
        "continueWatchingShowResumePromptOnLaunch" => "showResumePromptOnLaunch",
        "continueWatchingSortMode" => "sort_mode",
        _ => bail!("unknown Continue Watching setting"),
    };
    match key {
        "continueWatchingStyle" => {
            bail_unless(
                matches!(value.as_str(), Some("Card" | "Wide" | "Poster")),
                "unsupported Continue Watching style",
            )?;
        }
        "continueWatchingSortMode" => {
            bail_unless(
                matches!(
                    value.as_str(),
                    Some("DEFAULT" | "STREAMING_STYLE" | "SPLIT_UPCOMING")
                ),
                "unsupported Continue Watching sort mode",
            )?;
        }
        _ => {
            bail_unless(
                value.is_boolean(),
                "Continue Watching setting must be true or false",
            )?;
        }
    }
    payload.insert(storage_key.to_string(), value);
    let encoded = serde_json::to_string(&Value::Object(payload))
        .context("Continue Watching preferences could not be encoded")?;
    let root = blob
        .as_object_mut()
        .context("settings blob is not an object")?;
    object_entry(root, "features")?.insert(
        CONTINUE_WATCHING_PAYLOAD_KEY.to_string(),
        Value::String(encoded),
    );
    Ok(())
}

pub fn dismissed_next_up(blob: &Value) -> Vec<String> {
    normalized_dismissed_next_up_keys(&continue_watching_payload(blob))
}

/// Clears every dismissal for a title.
///
/// Mirrors `WatchProgressRepository.removeDismissedNextUpKeysForContent`,
/// which Nuvio calls whenever progress is recorded: resuming a show you had
/// dismissed should bring its suggestions back, otherwise the suppression is
/// permanent. Returns `None` when nothing was dismissed, so the caller can
/// skip a pointless push.
pub fn clear_dismissed_for_content(
    auth: &AuthService,
    profile_id: i32,
    current_blob: &Value,
    content_id: &str,
) -> Result<Option<(SettingsSnapshot, Value)>> {
    let prefix = format!("{}|", content_id.trim());
    let existing = dismissed_next_up(current_blob);
    if !existing.iter().any(|key| key.starts_with(&prefix)) {
        return Ok(None);
    }
    let mut blob = current_blob.clone();
    sanitize_profile_blob(&mut blob);
    let mut payload = normalized_continue_watching_payload(&continue_watching_payload(&blob));
    let kept: Vec<String> = existing
        .into_iter()
        .filter(|key| !key.starts_with(&prefix))
        .collect();
    payload.insert("dismissedNextUpKeys".to_string(), json!(kept));
    let encoded = serde_json::to_string(&Value::Object(payload))
        .context("continue watching preferences could not be encoded")?;
    let root = blob
        .as_object_mut()
        .context("settings blob is not an object")?;
    let features = object_entry(root, "features")?;
    features.insert(CONTINUE_WATCHING_PAYLOAD_KEY.to_string(), json!(encoded));
    auth.rpc_unit(
        "sync_push_profile_settings_blob",
        &json!({
            "p_profile_id": profile_id,
            "p_platform": "desktop",
            "p_settings_json": blob,
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )?;
    Ok(Some((snapshot(&blob), blob)))
}

/// Adds or removes a dismissed next-up key and pushes the blob.
pub fn set_next_up_dismissed(
    auth: &AuthService,
    profile_id: i32,
    current_blob: &Value,
    key: &str,
    dismissed: bool,
) -> Result<(SettingsSnapshot, Value)> {
    let mut blob = current_blob.clone();
    sanitize_profile_blob(&mut blob);
    let mut payload = normalized_continue_watching_payload(&continue_watching_payload(&blob));
    let mut keys: Vec<String> = payload
        .get("dismissedNextUpKeys")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    keys.retain(|existing| existing != key);
    if dismissed {
        keys.push(key.to_string());
    }
    payload.insert("dismissedNextUpKeys".to_string(), json!(keys));

    let encoded = serde_json::to_string(&Value::Object(payload))
        .context("continue watching preferences could not be encoded")?;
    let root = blob
        .as_object_mut()
        .context("settings blob is not an object")?;
    let features = object_entry(root, "features")?;
    features.insert(CONTINUE_WATCHING_PAYLOAD_KEY.to_string(), json!(encoded));

    auth.rpc_unit(
        "sync_push_profile_settings_blob",
        &json!({
            "p_profile_id": profile_id,
            "p_platform": "desktop",
            "p_settings_json": blob,
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )?;
    Ok((snapshot(&blob), blob))
}

fn poster_style(blob: &Value) -> Value {
    blob.pointer(&format!("/features/{POSTER_PAYLOAD_KEY}"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// Materialises every field Nuvio's `StoredPosterCardStylePreferences` carries.
/// Kotlin serializes with `encodeDefaults = true`, so a payload written here
/// should look exactly like one it wrote — and, critically, the hover-preview
/// fields must survive a desktop write untouched.
fn normalized_poster_style(style: &Value) -> Map<String, Value> {
    let read_i64 =
        |key: &str, fallback: i64| style.get(key).and_then(Value::as_i64).unwrap_or(fallback);
    let read_bool =
        |key: &str, fallback: bool| style.get(key).and_then(Value::as_bool).unwrap_or(fallback);
    let width = read_i64("widthDp", DEFAULT_POSTER_WIDTH);
    let mut object = Map::new();
    object.insert("widthDp".to_string(), json!(width));
    // Nuvio derives height from width (`width * 3 / 2`) and stores both. Writing
    // a mismatched pair would render a stretched card on the other clients.
    object.insert("heightDp".to_string(), json!(width * 3 / 2));
    object.insert(
        "cornerRadiusDp".to_string(),
        json!(read_i64("cornerRadiusDp", DEFAULT_POSTER_CORNER_RADIUS)),
    );
    object.insert(
        "catalogLandscapeModeEnabled".to_string(),
        json!(read_bool("catalogLandscapeModeEnabled", false)),
    );
    object.insert(
        "hideLabelsEnabled".to_string(),
        json!(read_bool("hideLabelsEnabled", false)),
    );
    object.insert(
        "hoverPreviewEnabled".to_string(),
        json!(read_bool("hoverPreviewEnabled", true)),
    );
    object.insert(
        "hoverPreviewOpenDelayMillis".to_string(),
        json!(read_i64("hoverPreviewOpenDelayMillis", 2_000)),
    );
    object.insert(
        "hoverPreviewTrailerEnabled".to_string(),
        json!(read_bool("hoverPreviewTrailerEnabled", false)),
    );
    object.insert(
        "hoverPreviewTrailerSoundEnabled".to_string(),
        json!(read_bool("hoverPreviewTrailerSoundEnabled", false)),
    );
    object.insert(
        "hoverPreviewTrailerStartSeconds".to_string(),
        json!(read_i64("hoverPreviewTrailerStartSeconds", 0)),
    );
    object
}

fn set_poster_style(blob: &mut Value, key: &str, value: Value) -> Result<()> {
    let mut style = normalized_poster_style(&poster_style(blob));
    match key {
        "posterWidth" => {
            let width = value
                .as_i64()
                .context("poster width must be a whole number")?;
            bail_unless(
                (80..=240).contains(&width),
                "poster width must be between 80 and 240",
            )?;
            style.insert("widthDp".to_string(), json!(width));
            style.insert("heightDp".to_string(), json!(width * 3 / 2));
        }
        "posterCornerRadius" => {
            let radius = value
                .as_i64()
                .context("corner radius must be a whole number")?;
            bail_unless(
                (0..=32).contains(&radius),
                "corner radius must be between 0 and 32",
            )?;
            style.insert("cornerRadiusDp".to_string(), json!(radius));
        }
        "posterHideLabels" => {
            style.insert(
                "hideLabelsEnabled".to_string(),
                json!(
                    value
                        .as_bool()
                        .context("hide labels must be true or false")?
                ),
            );
        }
        "posterLandscapeCatalogs" => {
            style.insert(
                "catalogLandscapeModeEnabled".to_string(),
                json!(
                    value
                        .as_bool()
                        .context("landscape posters must be true or false")?
                ),
            );
        }
        _ => bail!("unknown poster setting"),
    }

    let root = blob
        .as_object_mut()
        .context("settings blob is not an object")?;
    let features = object_entry(root, "features")?;
    features.insert(
        POSTER_PAYLOAD_KEY.to_string(),
        Value::String(serde_json::to_string(&Value::Object(style))?),
    );
    Ok(())
}

fn is_argb_hex(value: &str) -> bool {
    let body = value.strip_prefix('#').unwrap_or(value);
    matches!(body.len(), 6 | 8) && body.chars().all(|c| c.is_ascii_hexdigit())
}

fn bail_unless(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { bail!(message) }
}

pub fn load(auth: &AuthService, profile_id: i32) -> Result<(SettingsSnapshot, Value)> {
    let response = auth.rpc_value(
        "sync_pull_profile_settings_blob",
        &json!({ "p_profile_id": profile_id, "p_platform": "desktop" }),
    )?;
    let mut blob = response
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("settings_json"))
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(empty_blob);
    // Older prototype builds accidentally put provider secrets in this blob.
    // Strip both the official names and those legacy aliases before the value
    // can become the source for any subsequent whole-blob push.
    sanitize_profile_blob(&mut blob);
    Ok((snapshot(&blob), blob))
}

pub fn load_provider_credentials(
    auth: &AuthService,
    profile_id: i32,
) -> Result<ProviderCredentialStore> {
    // Official Nuvio seeds its complete local provider shape before pulling.
    // The seed RPC only inserts missing rows, so this makes fresh/self-hosted
    // profiles readable without replacing remote or future-provider rows.
    auth.rpc_unit(
        "sync_seed_provider_credentials",
        &required_provider_seed_params(profile_id, auth.sync_client_id()),
    )?;
    let response = auth.rpc_value(
        "sync_pull_provider_credentials",
        &json!({ "p_profile_id": profile_id }),
    )?;
    ProviderCredentialStore::from_rpc_value(&response)
}

fn required_provider_seed_params(profile_id: i32, origin_client_id: &str) -> Value {
    json!({
        "p_profile_id": profile_id,
        "p_credentials": [
            { "provider": "tmdb", "credential_json": { "api_key": "" } },
            { "provider": "mdblist", "credential_json": { "api_key": "" } },
            { "provider": "animeskip", "credential_json": { "client_id": "" } },
            { "provider": "introdb", "credential_json": { "api_key": "" } },
            { "provider": "debrid:torbox", "credential_json": { "api_key": "" } },
            { "provider": "debrid:premiumize", "credential_json": { "api_key": "" } },
            { "provider": "debrid:realdebrid", "credential_json": { "api_key": "" } },
        ],
        "p_origin_client_id": origin_client_id,
    })
}

/// Builds metadata behavior from the profile blob and the separately synced
/// provider credential cache. This function never performs I/O.
pub fn metadata_config_from_blob(
    blob: &Value,
    credentials: &ProviderCredentialStore,
) -> MetadataConfig {
    metadata_config_with_keys(
        blob,
        credentials.credential("tmdb", "api_key"),
        credentials.credential("mdblist", "api_key"),
    )
}

impl ProviderCredentialStore {
    fn from_rpc_value(value: &Value) -> Result<Self> {
        let rows = value
            .as_array()
            .context("provider credential response is not a list")?
            .iter()
            .map(|row| {
                let provider = row
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|provider| !provider.is_empty())
                    .context("provider credential row has no provider")?
                    .to_string();
                let credential_json = row
                    .get("credential_json")
                    .and_then(Value::as_object)
                    .cloned()
                    .context("provider credential payload is not an object")?;
                Ok(ProviderCredentialRow {
                    provider,
                    credential_json,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { rows })
    }

    fn credential(&self, provider: &str, field: &str) -> String {
        self.rows
            .iter()
            .find(|row| row.provider.eq_ignore_ascii_case(provider))
            .and_then(|row| row.credential_json.get(field))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    pub fn has_api_key(&self, provider: &str) -> bool {
        !self.credential(provider, "api_key").is_empty()
    }

    pub fn anime_skip_client_id(&self) -> String {
        self.credential("animeskip", "client_id")
    }

    pub fn intro_db_api_key(&self) -> String {
        self.credential("introdb", "api_key")
    }

    pub fn configured_debrid_resolver_provider_ids(&self) -> Vec<&'static str> {
        // This order is official Nuvio's provider registry order. Real-Debrid
        // has a synced credential row but is deliberately hidden/disabled by
        // the official provider policy, so it cannot become the active service.
        ["torbox", "premiumize"]
            .into_iter()
            .filter(|provider| self.has_api_key(&format!("debrid:{provider}")))
            .collect()
    }

    pub fn snapshot(&self) -> IntegrationCredentialSnapshot {
        IntegrationCredentialSnapshot {
            tmdb_api_key: self.credential("tmdb", "api_key"),
            mdb_list_api_key: self.credential("mdblist", "api_key"),
            anime_skip_client_id: self.anime_skip_client_id(),
            intro_db_api_key: self.intro_db_api_key(),
            torbox_api_key: self.credential("debrid:torbox", "api_key"),
            premiumize_api_key: self.credential("debrid:premiumize", "api_key"),
            real_debrid_api_key: self.credential("debrid:realdebrid", "api_key"),
        }
    }

    fn with_credential(&self, provider: &str, value: &str) -> Result<Self> {
        let field = match provider {
            "tmdb" | "mdblist" | "introdb" | "debrid:torbox" | "debrid:premiumize"
            | "debrid:realdebrid" => "api_key",
            "animeskip" => "client_id",
            _ => bail!("unsupported credential provider"),
        };
        let value = value.trim();
        bail_unless(
            value.len() <= 16 * 1024 && !value.contains(['\r', '\n']),
            "provider credential is invalid",
        )?;

        let mut next = self.clone();
        let mut found = false;
        for row in &mut next.rows {
            if row.provider.eq_ignore_ascii_case(provider) {
                row.credential_json.insert(field.to_string(), json!(value));
                found = true;
            }
        }
        if !found {
            next.rows.push(ProviderCredentialRow {
                provider: provider.to_string(),
                credential_json: Map::from_iter([(field.to_string(), json!(value))]),
            });
        }
        Ok(next)
    }

    fn push_params(&self, profile_id: i32, origin_client_id: &str) -> Value {
        json!({
            "p_profile_id": profile_id,
            "p_credentials": self.rows.iter().map(|row| json!({
                "provider": row.provider,
                "credential_json": row.credential_json,
            })).collect::<Vec<_>>(),
            "p_origin_client_id": origin_client_id,
        })
    }
}

/// Pull-before-merge is intentional: the RPC accepts the complete provider
/// array, so writing from a stale startup cache could erase credentials changed
/// on another device. Unknown providers and unknown fields are carried through.
pub fn update_provider_credential(
    auth: &AuthService,
    profile_id: i32,
    provider: &str,
    value: &str,
) -> Result<ProviderCredentialStore> {
    let current = load_provider_credentials(auth, profile_id)?;
    let next = current.with_credential(provider, value)?;
    auth.rpc_unit(
        "sync_push_provider_credentials",
        &next.push_params(profile_id, auth.sync_client_id()),
    )?;
    Ok(next)
}

/// Mirrors `DebridSettingsRepository.setProviderApiKey`: the active resolver
/// must always be a configured, visible provider and link resolving turns off
/// when the final resolver credential is removed. This is a single full-blob
/// write and leaves every unknown setting untouched.
pub fn normalize_debrid_settings_for_credentials(
    auth: &AuthService,
    profile_id: i32,
    current_blob: &Value,
    credentials: &ProviderCredentialStore,
) -> Result<(SettingsSnapshot, Value)> {
    let mut blob = current_blob.clone();
    sanitize_profile_blob(&mut blob);
    let configured = credentials.configured_debrid_resolver_provider_ids();
    let current_preferred = typed_string(
        &blob,
        "debrid_settings",
        "debrid_preferred_resolver_provider_id",
    )
    .unwrap_or_default();
    let normalized_preferred = if configured.contains(&current_preferred.as_str()) {
        current_preferred.clone()
    } else {
        configured.first().copied().unwrap_or_default().to_string()
    };
    let mut changed = false;
    if current_preferred != normalized_preferred {
        set_typed_preference(
            &mut blob,
            "debrid_settings",
            "debrid_preferred_resolver_provider_id",
            Kind::String,
            json!(normalized_preferred),
        )?;
        changed = true;
    }
    if configured.is_empty()
        && typed_bool(&blob, "debrid_settings", "debrid_enabled").unwrap_or(false)
    {
        set_typed_preference(
            &mut blob,
            "debrid_settings",
            "debrid_enabled",
            Kind::Boolean,
            json!(false),
        )?;
        changed = true;
    }
    if changed {
        auth.rpc_unit(
            "sync_push_profile_settings_blob",
            &json!({
                "p_profile_id": profile_id,
                "p_platform": "desktop",
                "p_settings_json": blob,
                "p_origin_client_id": auth.sync_client_id(),
            }),
        )?;
    }
    Ok((snapshot(&blob), blob))
}

fn metadata_config_with_keys(
    blob: &Value,
    tmdb_api_key: String,
    mdblist_api_key: String,
) -> MetadataConfig {
    MetadataConfig {
        tmdb: TmdbMetadataSettings {
            enabled: typed_bool(&blob, "tmdb_settings", "tmdb_enabled").unwrap_or(false)
                && !tmdb_api_key.is_empty(),
            api_key: tmdb_api_key,
            language: typed_string(&blob, "tmdb_settings", "tmdb_language")
                .unwrap_or_else(|| "en".to_string()),
            use_trailers: typed_bool(&blob, "tmdb_settings", "tmdb_use_trailers").unwrap_or(true),
            use_artwork: typed_bool(&blob, "tmdb_settings", "tmdb_use_artwork").unwrap_or(true),
            use_basic_info: typed_bool(&blob, "tmdb_settings", "tmdb_use_basic_info")
                .unwrap_or(true),
            use_details: typed_bool(&blob, "tmdb_settings", "tmdb_use_details").unwrap_or(true),
            use_release_dates: typed_bool(&blob, "tmdb_settings", "tmdb_use_release_dates")
                .unwrap_or(false),
            use_credits: typed_bool(&blob, "tmdb_settings", "tmdb_use_credits").unwrap_or(true),
            use_productions: typed_bool(&blob, "tmdb_settings", "tmdb_use_productions")
                .unwrap_or(true),
            use_networks: typed_bool(&blob, "tmdb_settings", "tmdb_use_networks").unwrap_or(true),
            use_episodes: typed_bool(&blob, "tmdb_settings", "tmdb_use_episodes").unwrap_or(true),
            use_season_posters: typed_bool(&blob, "tmdb_settings", "tmdb_use_season_posters")
                .unwrap_or(true),
            use_more_like_this: typed_bool(&blob, "tmdb_settings", "tmdb_use_more_like_this")
                .unwrap_or(true),
            use_collections: typed_bool(&blob, "tmdb_settings", "tmdb_use_collections")
                .unwrap_or(true),
        },
        mdblist: MdbListMetadataSettings {
            enabled: typed_bool(&blob, "mdblist_settings", "mdblist_enabled").unwrap_or(false)
                && !mdblist_api_key.is_empty(),
            api_key: mdblist_api_key,
            providers: [
                ("imdb", "mdblist_use_imdb"),
                ("tmdb", "mdblist_use_tmdb"),
                ("tomatoes", "mdblist_use_tomatoes"),
                ("metacritic", "mdblist_use_metacritic"),
                ("trakt", "mdblist_use_trakt"),
                ("letterboxd", "mdblist_use_letterboxd"),
                ("audience", "mdblist_use_audience"),
                ("mal", "mdblist_use_mal"),
            ]
            .into_iter()
            .filter(|(_, key)| typed_bool(&blob, "mdblist_settings", key).unwrap_or(true))
            .map(|(provider, _)| provider.to_string())
            .collect(),
        },
    }
}

/// Updates a caller-owned profile blob and returns the new cache contents.
/// The remote write happens before the new value is exposed, so a failed sync
/// cannot make the UI claim a setting was saved when it was not.
pub fn update_cached(
    auth: &AuthService,
    profile_id: i32,
    current_blob: &Value,
    key: &str,
    value: Value,
) -> Result<(SettingsSnapshot, Value)> {
    let mut blob = current_blob.clone();
    // A full-blob push must never re-upload credentials left behind by an
    // older prototype build. Official Nuvio syncs these through the dedicated
    // provider-credentials RPC instead.
    sanitize_profile_blob(&mut blob);
    if key.starts_with("poster") {
        set_poster_style(&mut blob, key, value)?;
    } else if key.starts_with("continueWatching") {
        set_continue_watching_setting(&mut blob, key, value)?;
    } else if key == "episodeReleaseAlerts" {
        let enabled = value
            .as_bool()
            .context("episodeReleaseAlerts must be true or false")?;
        let root = blob
            .as_object_mut()
            .context("settings blob is not an object")?;
        let features = object_entry(root, "features")?;
        let notifications = object_entry(features, "notifications_settings")?;
        notifications.insert("episode_release_alerts_enabled".to_string(), json!(enabled));
    } else {
        let (feature, storage_key, kind) =
            setting_path(key).context("unknown or unsupported setting")?;
        let normalized = validate_value(key, value, kind)?;
        set_typed_preference(&mut blob, feature, storage_key, kind, normalized.clone())?;
        sync_legacy_debrid_stream_preference(&mut blob, key, &normalized)?;
    }
    auth.rpc_unit(
        "sync_push_profile_settings_blob",
        &json!({
            "p_profile_id": profile_id,
            "p_platform": "desktop",
            "p_settings_json": blob,
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )?;
    Ok((snapshot(&blob), blob))
}

/// Read-only launch overrides from the shared UI. Its auth transport writes
/// settings independently of the legacy shell cache. Restrict this to player
/// preferences and never persist the merged value or touch another feature.
pub(crate) fn playback_snapshot(base: Option<&Value>, preferences: &serde_json::Map<String, Value>) -> SettingsSnapshot {
    let mut features = base.and_then(|blob| blob.get("features")).and_then(Value::as_object).cloned().unwrap_or_default();
    let mut player = features.get("player_settings").and_then(Value::as_object).cloned().unwrap_or_default();
    for key in [
        "resize_mode", "preferred_audio_language", "secondary_preferred_audio_language",
        "preferred_subtitle_language", "secondary_preferred_subtitle_language",
        "subtitle_text_color", "subtitle_background_color", "subtitle_outline_color",
        "subtitle_font_size_sp", "subtitle_bottom_offset", "subtitle_outline_width",
        "subtitle_bold", "subtitle_outline_enabled", "subtitle_use_forced_subtitles",
        "subtitle_show_only_preferred_languages", "use_libass", "nvidia_rtx_super_resolution_enabled",
    ] {
        if let Some(value) = preferences.get(key) { player.insert(key.to_string(), value.clone()); }
    }
    features.insert("player_settings".to_string(), Value::Object(player));
    snapshot(&json!({ "version": 3, "features": features }))
}

#[cfg(test)]
mod playback_snapshot_tests {
    use super::*;

    #[test]
    fn current_ui_preferences_override_stale_cache_without_mutating_it() {
        let base = json!({"features":{"player_settings":{
            "subtitle_bold":{"type":"boolean","value":false},
            "nvidia_rtx_super_resolution_enabled":{"type":"boolean","value":true}
        }}});
        let original = base.clone();
        let preferences = json!({
            "subtitle_bold":{"type":"boolean","value":true},
            "use_libass":{"type":"boolean","value":true},
            "preferred_audio_language":{"type":"string","value":"en"},
            "nvidia_rtx_super_resolution_enabled":{"type":"boolean","value":false}
        });
        let result = playback_snapshot(Some(&base), preferences.as_object().unwrap());
        assert!(result.subtitle_bold);
        assert!(result.use_libass);
        assert_eq!(result.preferred_audio_language, "en");
        assert!(!result.rtx_super_resolution);
        assert_eq!(base, original);
    }

    #[test]
    fn rtx_launch_preference_overrides_both_stale_states_and_an_empty_cache() {
        for enabled in [true, false] {
            let base = json!({"features":{"player_settings":{
                "nvidia_rtx_super_resolution_enabled":{"type":"boolean","value":!enabled}
            }}});
            let preferences = json!({
                "nvidia_rtx_super_resolution_enabled":{"type":"boolean","value":enabled}
            });
            for cached in [Some(&base), None] {
                assert_eq!(playback_snapshot(cached, preferences.as_object().unwrap()).rtx_super_resolution, enabled);
            }
            assert_eq!(playback_snapshot(Some(&base), &serde_json::Map::new()).rtx_super_resolution, !enabled);
        }
    }
}

pub(crate) fn snapshot(blob: &Value) -> SettingsSnapshot {
    let continue_watching = normalized_continue_watching_payload(&continue_watching_payload(blob));
    SettingsSnapshot {
        amoled_enabled: typed_bool(blob, "theme_settings", "amoled_enabled").unwrap_or(false),
        continue_watching_visible: continue_watching["isVisible"].as_bool().unwrap_or(true),
        continue_watching_style: continue_watching["style"]
            .as_str()
            .unwrap_or("Card")
            .to_string(),
        continue_watching_up_next_from_furthest_episode:
            continue_watching["upNextFromFurthestEpisode"]
                .as_bool()
                .unwrap_or(true),
        continue_watching_use_episode_thumbnails: continue_watching["use_episode_thumbnails_in_cw"]
            .as_bool()
            .unwrap_or(true),
        continue_watching_show_unaired_next_up: continue_watching["show_unaired_next_up"]
            .as_bool()
            .unwrap_or(true),
        continue_watching_blur_next_up: continue_watching["blur_continue_watching_next_up"]
            .as_bool()
            .unwrap_or(false),
        dismissed_next_up: dismissed_next_up(blob),
        continue_watching_show_resume_prompt_on_launch:
            continue_watching["showResumePromptOnLaunch"]
                .as_bool()
                .unwrap_or(true),
        continue_watching_sort_mode: continue_watching["sort_mode"]
            .as_str()
            .unwrap_or("DEFAULT")
            .to_string(),
        show_loading_overlay: typed_bool(blob, "player_settings", "show_loading_overlay")
            .unwrap_or(true),
        show_parental_guide: typed_bool(blob, "player_settings", "show_parental_guide")
            .unwrap_or(true),
        // Official desktop treats the mobile-only Fill mode as Zoom.
        resize_mode: match typed_string(blob, "player_settings", "resize_mode").as_deref() {
            Some("Fill") => "Zoom".to_string(),
            Some(value) => value.to_string(),
            None => "Fit".to_string(),
        },
        preferred_audio_language: typed_string(blob, "player_settings", "preferred_audio_language")
            .unwrap_or_else(|| "device".to_string()),
        preferred_subtitle_language: typed_string(
            blob,
            "player_settings",
            "preferred_subtitle_language",
        )
        .unwrap_or_else(|| "none".to_string()),
        subtitle_bold: typed_bool(blob, "player_settings", "subtitle_bold").unwrap_or(false),
        subtitle_font_size: typed_i64(blob, "player_settings", "subtitle_font_size_sp")
            .unwrap_or(18),
        subtitle_outline: typed_bool(blob, "player_settings", "subtitle_outline_enabled")
            .unwrap_or(true),
        reuse_last_stream: typed_bool(blob, "player_settings", "stream_reuse_last_link_enabled")
            .unwrap_or(false),
        reuse_last_stream_hours: typed_i64(
            blob,
            "player_settings",
            "stream_reuse_last_link_cache_hours",
        )
        .unwrap_or(24),
        autoplay_mode: typed_string(blob, "player_settings", "stream_auto_play_mode")
            .unwrap_or_else(|| "MANUAL".to_string()),
        autoplay_next_episode: typed_bool(
            blob,
            "player_settings",
            "stream_auto_play_next_episode_enabled",
        )
        .unwrap_or(false),
        skip_intro: typed_bool(blob, "player_settings", "skip_intro_enabled").unwrap_or(true),
        rtx_super_resolution: typed_bool(
            blob,
            "player_settings",
            "nvidia_rtx_super_resolution_enabled",
        )
        .unwrap_or(false),
        show_file_size_badges: typed_bool(blob, "stream_badge_settings", "show_file_size_badges")
            .unwrap_or(true),
        badge_placement: typed_string(blob, "stream_badge_settings", "stream_badge_placement")
            .unwrap_or_else(|| "BOTTOM".to_string()),
        episode_release_alerts: blob
            .pointer("/features/notifications_settings/episode_release_alerts_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        poster_width: poster_style(blob)
            .get("widthDp")
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_POSTER_WIDTH),
        poster_corner_radius: poster_style(blob)
            .get("cornerRadiusDp")
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_POSTER_CORNER_RADIUS),
        poster_hide_labels: poster_style(blob)
            .get("hideLabelsEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        poster_landscape_catalogs: poster_style(blob)
            .get("catalogLandscapeModeEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_episode_threshold_mode: typed_string(
            blob,
            "player_settings",
            "next_episode_threshold_mode",
        )
        .unwrap_or_else(|| "PERCENTAGE".to_string()),
        next_episode_threshold_percent: typed_f64(
            blob,
            "player_settings",
            "next_episode_threshold_percent_v2",
        )
        .unwrap_or(99.0),
        next_episode_threshold_minutes: typed_f64(
            blob,
            "player_settings",
            "next_episode_threshold_minutes_before_end_v2",
        )
        .unwrap_or(2.0),
        subtitle_text_color: player_string(blob, "subtitle_text_color", "#FFFFFFFF"),
        subtitle_background_color: player_string(blob, "subtitle_background_color", "#00000000"),
        subtitle_outline_color: player_string(blob, "subtitle_outline_color", "#FF000000"),
        subtitle_outline_width: player_int(blob, "subtitle_outline_width", 2),
        subtitle_bottom_offset: player_int(blob, "subtitle_bottom_offset", 20),
        subtitle_forced_only: player_bool(blob, "subtitle_use_forced_subtitles", false),
        subtitle_preferred_languages_only: player_bool(
            blob,
            "subtitle_show_only_preferred_languages",
            false,
        ),
        secondary_audio_language: player_string(blob, "secondary_preferred_audio_language", ""),
        secondary_subtitle_language: player_string(
            blob,
            "secondary_preferred_subtitle_language",
            "",
        ),
        addon_subtitle_startup_mode: player_string(
            blob,
            "addon_subtitle_startup_mode",
            "ALL_SUBTITLES",
        ),
        use_libass: player_bool(blob, "use_libass", false),
        autoplay_source: player_string(blob, "stream_auto_play_source", "ALL_SOURCES"),
        autoplay_selected_addons: player_string_set(blob, "stream_auto_play_selected_addons"),
        autoplay_selected_plugins: player_string_set(blob, "stream_auto_play_selected_plugins"),
        autoplay_regex: player_string(blob, "stream_auto_play_regex", ""),
        autoplay_timeout_seconds: player_int(blob, "stream_auto_play_timeout_seconds", 3),
        autoplay_prefer_binge_group: player_bool(blob, "stream_auto_play_prefer_binge_group", true),
        autoplay_reuse_binge_group: player_bool(blob, "stream_auto_play_reuse_binge_group", true),
        autoplay_next_episode_fallback: player_bool(
            blob,
            "stream_auto_play_next_episode_fallback_enabled",
            true,
        ),
        anime_skip_enabled: player_bool(blob, "animeskip_enabled", false),
        hold_to_speed: player_bool(blob, "hold_to_speed_enabled", true),
        hold_to_speed_value: typed_f64(blob, "player_settings", "hold_to_speed_value")
            .unwrap_or(2.0),
        external_player_enabled: player_bool(blob, "external_player_enabled", false),
        external_player_id: player_string(blob, "external_player_id", ""),
        external_player_forward_subtitles: player_bool(
            blob,
            "external_player_forward_subtitles",
            false,
        ),
        external_player_send_skip_segments: player_bool(
            blob,
            "external_player_send_skip_segments",
            false,
        ),
        tmdb_enabled: typed_bool(blob, "tmdb_settings", "tmdb_enabled").unwrap_or(false),
        tmdb_language: typed_string(blob, "tmdb_settings", "tmdb_language")
            .unwrap_or_else(|| "en".to_string()),
        tmdb_use_trailers: typed_bool(blob, "tmdb_settings", "tmdb_use_trailers").unwrap_or(true),
        tmdb_use_artwork: typed_bool(blob, "tmdb_settings", "tmdb_use_artwork").unwrap_or(true),
        tmdb_use_basic_info: typed_bool(blob, "tmdb_settings", "tmdb_use_basic_info")
            .unwrap_or(true),
        tmdb_use_details: typed_bool(blob, "tmdb_settings", "tmdb_use_details").unwrap_or(true),
        tmdb_use_release_dates: typed_bool(blob, "tmdb_settings", "tmdb_use_release_dates")
            .unwrap_or(false),
        tmdb_use_credits: typed_bool(blob, "tmdb_settings", "tmdb_use_credits").unwrap_or(true),
        tmdb_use_productions: typed_bool(blob, "tmdb_settings", "tmdb_use_productions")
            .unwrap_or(true),
        tmdb_use_networks: typed_bool(blob, "tmdb_settings", "tmdb_use_networks").unwrap_or(true),
        tmdb_use_episodes: typed_bool(blob, "tmdb_settings", "tmdb_use_episodes").unwrap_or(true),
        tmdb_use_season_posters: typed_bool(blob, "tmdb_settings", "tmdb_use_season_posters")
            .unwrap_or(true),
        tmdb_use_more_like_this: typed_bool(blob, "tmdb_settings", "tmdb_use_more_like_this")
            .unwrap_or(true),
        tmdb_use_collections: typed_bool(blob, "tmdb_settings", "tmdb_use_collections")
            .unwrap_or(true),
        mdb_list_enabled: typed_bool(blob, "mdblist_settings", "mdblist_enabled").unwrap_or(false),
        mdb_list_use_imdb: typed_bool(blob, "mdblist_settings", "mdblist_use_imdb").unwrap_or(true),
        mdb_list_use_tmdb: typed_bool(blob, "mdblist_settings", "mdblist_use_tmdb").unwrap_or(true),
        mdb_list_use_tomatoes: typed_bool(blob, "mdblist_settings", "mdblist_use_tomatoes")
            .unwrap_or(true),
        mdb_list_use_metacritic: typed_bool(blob, "mdblist_settings", "mdblist_use_metacritic")
            .unwrap_or(true),
        mdb_list_use_trakt: typed_bool(blob, "mdblist_settings", "mdblist_use_trakt")
            .unwrap_or(true),
        mdb_list_use_letterboxd: typed_bool(blob, "mdblist_settings", "mdblist_use_letterboxd")
            .unwrap_or(true),
        mdb_list_use_audience: typed_bool(blob, "mdblist_settings", "mdblist_use_audience")
            .unwrap_or(true),
        mdb_list_use_mal: typed_bool(blob, "mdblist_settings", "mdblist_use_mal").unwrap_or(true),
        debrid_enabled: typed_bool(blob, "debrid_settings", "debrid_enabled").unwrap_or(false),
        debrid_cloud_library_enabled: typed_bool(
            blob,
            "debrid_settings",
            "debrid_cloud_library_enabled",
        )
        .unwrap_or(true),
        debrid_preferred_resolver_provider_id: typed_string(
            blob,
            "debrid_settings",
            "debrid_preferred_resolver_provider_id",
        )
        .unwrap_or_default(),
        debrid_instant_playback_preparation_limit: typed_i64(
            blob,
            "debrid_settings",
            "debrid_instant_playback_preparation_limit",
        )
        .unwrap_or(0),
        debrid_stream_max_results: typed_i64(blob, "debrid_settings", "debrid_stream_max_results")
            .unwrap_or(0),
        debrid_stream_sort_mode: typed_string(blob, "debrid_settings", "debrid_stream_sort_mode")
            .unwrap_or_else(|| "DEFAULT".to_string()),
        debrid_stream_minimum_quality: typed_string(
            blob,
            "debrid_settings",
            "debrid_stream_minimum_quality",
        )
        .unwrap_or_else(|| "ANY".to_string()),
        debrid_stream_dolby_vision_filter: typed_string(
            blob,
            "debrid_settings",
            "debrid_stream_dolby_vision_filter",
        )
        .unwrap_or_else(|| "ANY".to_string()),
        debrid_stream_hdr_filter: typed_string(blob, "debrid_settings", "debrid_stream_hdr_filter")
            .unwrap_or_else(|| "ANY".to_string()),
        debrid_stream_codec_filter: typed_string(
            blob,
            "debrid_settings",
            "debrid_stream_codec_filter",
        )
        .unwrap_or_else(|| "ANY".to_string()),
        debrid_stream_preferences: typed_string(
            blob,
            "debrid_settings",
            "debrid_stream_preferences",
        )
        .unwrap_or_default(),
        debrid_stream_name_template: typed_string(
            blob,
            "debrid_settings",
            "debrid_stream_name_template",
        )
        .unwrap_or_else(|| DEFAULT_DEBRID_STREAM_NAME_TEMPLATE.to_string()),
        debrid_stream_description_template: typed_string(
            blob,
            "debrid_settings",
            "debrid_stream_description_template",
        )
        .unwrap_or_default(),
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Boolean,
    String,
    Int,
    Float,
}

fn setting_path(key: &str) -> Option<(&'static str, &'static str, Kind)> {
    Some(match key {
        "amoledEnabled" => ("theme_settings", "amoled_enabled", Kind::Boolean),
        "showLoadingOverlay" => ("player_settings", "show_loading_overlay", Kind::Boolean),
        "showParentalGuide" => ("player_settings", "show_parental_guide", Kind::Boolean),
        "resizeMode" => ("player_settings", "resize_mode", Kind::String),
        "preferredAudioLanguage" => ("player_settings", "preferred_audio_language", Kind::String),
        "preferredSubtitleLanguage" => (
            "player_settings",
            "preferred_subtitle_language",
            Kind::String,
        ),
        "subtitleBold" => ("player_settings", "subtitle_bold", Kind::Boolean),
        "subtitleFontSize" => ("player_settings", "subtitle_font_size_sp", Kind::Int),
        "subtitleOutline" => ("player_settings", "subtitle_outline_enabled", Kind::Boolean),
        "reuseLastStream" => (
            "player_settings",
            "stream_reuse_last_link_enabled",
            Kind::Boolean,
        ),
        "autoplayMode" => ("player_settings", "stream_auto_play_mode", Kind::String),
        "autoplayNextEpisode" => (
            "player_settings",
            "stream_auto_play_next_episode_enabled",
            Kind::Boolean,
        ),
        "skipIntro" => ("player_settings", "skip_intro_enabled", Kind::Boolean),
        "subtitleTextColor" => ("player_settings", "subtitle_text_color", Kind::String),
        "subtitleBackgroundColor" => ("player_settings", "subtitle_background_color", Kind::String),
        "subtitleOutlineColor" => ("player_settings", "subtitle_outline_color", Kind::String),
        "subtitleOutlineWidth" => ("player_settings", "subtitle_outline_width", Kind::Int),
        "subtitleBottomOffset" => ("player_settings", "subtitle_bottom_offset", Kind::Int),
        "subtitleForcedOnly" => (
            "player_settings",
            "subtitle_use_forced_subtitles",
            Kind::Boolean,
        ),
        "subtitlePreferredLanguagesOnly" => (
            "player_settings",
            "subtitle_show_only_preferred_languages",
            Kind::Boolean,
        ),
        "secondaryAudioLanguage" => (
            "player_settings",
            "secondary_preferred_audio_language",
            Kind::String,
        ),
        "secondarySubtitleLanguage" => (
            "player_settings",
            "secondary_preferred_subtitle_language",
            Kind::String,
        ),
        "addonSubtitleStartupMode" => (
            "player_settings",
            "addon_subtitle_startup_mode",
            Kind::String,
        ),
        "useLibass" => ("player_settings", "use_libass", Kind::Boolean),
        "autoplaySource" => ("player_settings", "stream_auto_play_source", Kind::String),
        "autoplayRegex" => ("player_settings", "stream_auto_play_regex", Kind::String),
        "autoplayTimeoutSeconds" => (
            "player_settings",
            "stream_auto_play_timeout_seconds",
            Kind::Int,
        ),
        "autoplayPreferBingeGroup" => (
            "player_settings",
            "stream_auto_play_prefer_binge_group",
            Kind::Boolean,
        ),
        "autoplayReuseBingeGroup" => (
            "player_settings",
            "stream_auto_play_reuse_binge_group",
            Kind::Boolean,
        ),
        "autoplayNextEpisodeFallback" => (
            "player_settings",
            "stream_auto_play_next_episode_fallback_enabled",
            Kind::Boolean,
        ),
        "animeSkipEnabled" => ("player_settings", "animeskip_enabled", Kind::Boolean),
        "holdToSpeed" => ("player_settings", "hold_to_speed_enabled", Kind::Boolean),
        "holdToSpeedValue" => ("player_settings", "hold_to_speed_value", Kind::Float),
        "externalPlayerEnabled" => ("player_settings", "external_player_enabled", Kind::Boolean),
        "externalPlayerId" => ("player_settings", "external_player_id", Kind::String),
        "externalPlayerForwardSubtitles" => (
            "player_settings",
            "external_player_forward_subtitles",
            Kind::Boolean,
        ),
        "externalPlayerSendSkipSegments" => (
            "player_settings",
            "external_player_send_skip_segments",
            Kind::Boolean,
        ),
        "nextEpisodeThresholdMode" => (
            "player_settings",
            "next_episode_threshold_mode",
            Kind::String,
        ),
        "nextEpisodeThresholdPercent" => (
            "player_settings",
            "next_episode_threshold_percent_v2",
            Kind::Float,
        ),
        "nextEpisodeThresholdMinutes" => (
            "player_settings",
            "next_episode_threshold_minutes_before_end_v2",
            Kind::Float,
        ),
        "reuseLastStreamHours" => (
            "player_settings",
            "stream_reuse_last_link_cache_hours",
            Kind::Int,
        ),
        "rtxSuperResolution" => (
            "player_settings",
            "nvidia_rtx_super_resolution_enabled",
            Kind::Boolean,
        ),
        "showFileSizeBadges" => (
            "stream_badge_settings",
            "show_file_size_badges",
            Kind::Boolean,
        ),
        "badgePlacement" => (
            "stream_badge_settings",
            "stream_badge_placement",
            Kind::String,
        ),
        "tmdbEnabled" => ("tmdb_settings", "tmdb_enabled", Kind::Boolean),
        "tmdbLanguage" => ("tmdb_settings", "tmdb_language", Kind::String),
        "tmdbUseTrailers" => ("tmdb_settings", "tmdb_use_trailers", Kind::Boolean),
        "tmdbUseArtwork" => ("tmdb_settings", "tmdb_use_artwork", Kind::Boolean),
        "tmdbUseBasicInfo" => ("tmdb_settings", "tmdb_use_basic_info", Kind::Boolean),
        "tmdbUseDetails" => ("tmdb_settings", "tmdb_use_details", Kind::Boolean),
        "tmdbUseReleaseDates" => ("tmdb_settings", "tmdb_use_release_dates", Kind::Boolean),
        "tmdbUseCredits" => ("tmdb_settings", "tmdb_use_credits", Kind::Boolean),
        "tmdbUseProductions" => ("tmdb_settings", "tmdb_use_productions", Kind::Boolean),
        "tmdbUseNetworks" => ("tmdb_settings", "tmdb_use_networks", Kind::Boolean),
        "tmdbUseEpisodes" => ("tmdb_settings", "tmdb_use_episodes", Kind::Boolean),
        "tmdbUseSeasonPosters" => ("tmdb_settings", "tmdb_use_season_posters", Kind::Boolean),
        "tmdbUseMoreLikeThis" => ("tmdb_settings", "tmdb_use_more_like_this", Kind::Boolean),
        "tmdbUseCollections" => ("tmdb_settings", "tmdb_use_collections", Kind::Boolean),
        "mdbListEnabled" => ("mdblist_settings", "mdblist_enabled", Kind::Boolean),
        "mdbListUseImdb" => ("mdblist_settings", "mdblist_use_imdb", Kind::Boolean),
        "mdbListUseTmdb" => ("mdblist_settings", "mdblist_use_tmdb", Kind::Boolean),
        "mdbListUseTomatoes" => ("mdblist_settings", "mdblist_use_tomatoes", Kind::Boolean),
        "mdbListUseMetacritic" => ("mdblist_settings", "mdblist_use_metacritic", Kind::Boolean),
        "mdbListUseTrakt" => ("mdblist_settings", "mdblist_use_trakt", Kind::Boolean),
        "mdbListUseLetterboxd" => ("mdblist_settings", "mdblist_use_letterboxd", Kind::Boolean),
        "mdbListUseAudience" => ("mdblist_settings", "mdblist_use_audience", Kind::Boolean),
        "mdbListUseMal" => ("mdblist_settings", "mdblist_use_mal", Kind::Boolean),
        "debridEnabled" => ("debrid_settings", "debrid_enabled", Kind::Boolean),
        "debridCloudLibraryEnabled" => (
            "debrid_settings",
            "debrid_cloud_library_enabled",
            Kind::Boolean,
        ),
        "debridPreferredResolverProviderId" => (
            "debrid_settings",
            "debrid_preferred_resolver_provider_id",
            Kind::String,
        ),
        "debridInstantPlaybackPreparationLimit" => (
            "debrid_settings",
            "debrid_instant_playback_preparation_limit",
            Kind::Int,
        ),
        "debridStreamMaxResults" => ("debrid_settings", "debrid_stream_max_results", Kind::Int),
        "debridStreamSortMode" => ("debrid_settings", "debrid_stream_sort_mode", Kind::String),
        "debridStreamMinimumQuality" => (
            "debrid_settings",
            "debrid_stream_minimum_quality",
            Kind::String,
        ),
        "debridStreamDolbyVisionFilter" => (
            "debrid_settings",
            "debrid_stream_dolby_vision_filter",
            Kind::String,
        ),
        "debridStreamHdrFilter" => ("debrid_settings", "debrid_stream_hdr_filter", Kind::String),
        "debridStreamCodecFilter" => (
            "debrid_settings",
            "debrid_stream_codec_filter",
            Kind::String,
        ),
        "debridStreamPreferences" => ("debrid_settings", "debrid_stream_preferences", Kind::String),
        "debridStreamNameTemplate" => (
            "debrid_settings",
            "debrid_stream_name_template",
            Kind::String,
        ),
        "debridStreamDescriptionTemplate" => (
            "debrid_settings",
            "debrid_stream_description_template",
            Kind::String,
        ),
        _ => return None,
    })
}

fn validate_value(key: &str, value: Value, kind: Kind) -> Result<Value> {
    match kind {
        Kind::Boolean if !value.is_boolean() => bail!("{key} must be true or false"),
        Kind::String if !value.is_string() => bail!("{key} must be text"),
        Kind::Int if value.as_i64().is_none() => bail!("{key} must be a whole number"),
        Kind::Float if value.as_f64().is_none() => bail!("{key} must be a number"),
        _ => {}
    }
    let value = if key == "tmdbLanguage" {
        let language = value.as_str().unwrap_or_default().trim().replace('_', "-");
        bail_unless(
            language.len() <= 32 && !language.contains(['\r', '\n']),
            "invalid TMDB language code",
        )?;
        json!(language)
    } else {
        value
    };
    match key {
        "resizeMode" if !matches!(value.as_str(), Some("Fit" | "Fill" | "Zoom" | "Stretch")) => {
            bail!("unsupported resize mode")
        }
        "autoplayMode"
            if !matches!(
                value.as_str(),
                Some("MANUAL" | "FIRST_STREAM" | "REGEX_MATCH")
            ) =>
        {
            bail!("unsupported autoplay mode")
        }
        "badgePlacement" if !matches!(value.as_str(), Some("TOP" | "BOTTOM")) => {
            bail!("unsupported badge placement")
        }
        "subtitleFontSize" if !(6..=40).contains(&value.as_i64().unwrap_or_default()) => {
            bail!("subtitle size must be between 6 and 40")
        }
        "reuseLastStreamHours" if !(1..=720).contains(&value.as_i64().unwrap_or_default()) => {
            bail!("link reuse window must be between 1 and 720 hours")
        }
        // Nuvio clamps these to the same ranges before use.
        "subtitleOutlineWidth" if !(0..=8).contains(&value.as_i64().unwrap_or_default()) => {
            bail!("outline width must be between 0 and 8")
        }
        "subtitleBottomOffset" if !(0..=200).contains(&value.as_i64().unwrap_or_default()) => {
            bail!("subtitle offset must be between 0 and 200")
        }
        "autoplayTimeoutSeconds" if !(0..=30).contains(&value.as_i64().unwrap_or_default()) => {
            bail!("autoplay timeout must be between 0 and 30 seconds")
        }
        "holdToSpeedValue" if !(1.25..=4.0).contains(&value.as_f64().unwrap_or_default()) => {
            bail!("hold speed must be between 1.25x and 4x")
        }
        "autoplaySource"
            if !matches!(
                value.as_str(),
                Some(
                    "ALL_SOURCES"
                        | "INSTALLED_ADDONS_ONLY"
                        | "ENABLED_PLUGINS_ONLY"
                        // Read older prototype values long enough for existing
                        // profiles to migrate through the settings UI.
                        | "ADDONS_ONLY"
                        | "PLUGINS_ONLY"
                        | "SELECTED"
                )
            ) =>
        {
            bail!("unsupported autoplay source")
        }
        "addonSubtitleStartupMode"
            if !matches!(
                value.as_str(),
                Some("FAST_STARTUP" | "PREFERRED_ONLY" | "ALL_SUBTITLES")
            ) =>
        {
            bail!("unsupported subtitle startup mode")
        }
        // Colours ride as #AARRGGBB strings, matching Nuvio's storage format.
        "subtitleTextColor" | "subtitleBackgroundColor" | "subtitleOutlineColor"
            if !is_argb_hex(value.as_str().unwrap_or_default()) =>
        {
            bail!("colour must be #AARRGGBB")
        }
        "nextEpisodeThresholdMode"
            if !matches!(value.as_str(), Some("PERCENTAGE" | "MINUTES_BEFORE_END")) =>
        {
            bail!("unsupported next episode threshold mode")
        }
        "nextEpisodeThresholdPercent"
            if !(97.0..=100.0).contains(&value.as_f64().unwrap_or_default()) =>
        {
            bail!("next episode percentage must be between 97 and 100")
        }
        "nextEpisodeThresholdMinutes"
            if !(0.0..=3.5).contains(&value.as_f64().unwrap_or_default()) =>
        {
            bail!("next episode minutes must be between 0 and 3.5")
        }
        "debridPreferredResolverProviderId"
            if !matches!(value.as_str(), Some("" | "torbox" | "premiumize")) =>
        {
            bail!("unsupported debrid resolver provider")
        }
        "debridInstantPlaybackPreparationLimit"
            if !(0..=5).contains(&value.as_i64().unwrap_or_default()) =>
        {
            bail!("instant preparation limit must be between 0 and 5")
        }
        "debridStreamMaxResults" if !(0..=100).contains(&value.as_i64().unwrap_or_default()) => {
            bail!("debrid result limit must be between 0 and 100")
        }
        "debridStreamSortMode"
            if !matches!(
                value.as_str(),
                Some("DEFAULT" | "QUALITY_DESC" | "SIZE_DESC" | "SIZE_ASC")
            ) =>
        {
            bail!("unsupported debrid stream sort mode")
        }
        "debridStreamMinimumQuality"
            if !matches!(value.as_str(), Some("ANY" | "P720" | "P1080" | "P2160")) =>
        {
            bail!("unsupported minimum debrid quality")
        }
        "debridStreamDolbyVisionFilter" | "debridStreamHdrFilter"
            if !matches!(value.as_str(), Some("ANY" | "EXCLUDE" | "ONLY")) =>
        {
            bail!("unsupported debrid feature filter")
        }
        "debridStreamCodecFilter"
            if !matches!(value.as_str(), Some("ANY" | "H264" | "HEVC" | "AV1")) =>
        {
            bail!("unsupported debrid codec filter")
        }
        "debridStreamPreferences" => {
            let text = value.as_str().unwrap_or_default();
            bail_unless(text.len() <= 256 * 1024, "debrid preferences are too large")?;
            if !text.is_empty() {
                bail_unless(
                    serde_json::from_str::<Value>(text).is_ok_and(|value| value.is_object()),
                    "debrid preferences must be a JSON object",
                )?;
            }
        }
        "debridStreamNameTemplate" | "debridStreamDescriptionTemplate"
            if value.as_str().is_some_and(|text| text.len() > 64 * 1024) =>
        {
            bail!("debrid stream template is too large")
        }
        "preferredAudioLanguage" | "preferredSubtitleLanguage"
            if value.as_str().is_some_and(|text| text.len() > 16) =>
        {
            bail!("invalid language code")
        }
        _ => {}
    }
    Ok(value)
}

/// Official Nuvio keeps the old simple Debrid controls and the newer JSON
/// preference model in sync. Updating only the legacy key would appear to save
/// here but be ignored by Nuvio after its next load, so mutate the matching JSON
/// field while preserving every unrecognised preference.
fn sync_legacy_debrid_stream_preference(blob: &mut Value, key: &str, value: &Value) -> Result<()> {
    if !matches!(
        key,
        "debridStreamMaxResults"
            | "debridStreamSortMode"
            | "debridStreamMinimumQuality"
            | "debridStreamDolbyVisionFilter"
            | "debridStreamHdrFilter"
            | "debridStreamCodecFilter"
    ) {
        return Ok(());
    }
    let mut preferences = typed_string(blob, "debrid_settings", "debrid_stream_preferences")
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();

    match key {
        "debridStreamMaxResults" => {
            preferences.insert("maxResults".to_string(), value.clone());
        }
        "debridStreamSortMode" => {
            let criteria = match value.as_str().unwrap_or("DEFAULT") {
                "QUALITY_DESC" => json!([
                    { "key": "RESOLUTION", "direction": "DESC" },
                    { "key": "QUALITY", "direction": "DESC" },
                    { "key": "SIZE", "direction": "DESC" }
                ]),
                "SIZE_DESC" => json!([{ "key": "SIZE", "direction": "DESC" }]),
                "SIZE_ASC" => json!([{ "key": "SIZE", "direction": "ASC" }]),
                _ => json!([]),
            };
            preferences.insert("sortCriteria".to_string(), criteria);
        }
        "debridStreamMinimumQuality" => {
            let resolutions = match value.as_str().unwrap_or("ANY") {
                "P720" => json!(["P2160", "P1440", "P1080", "P720"]),
                "P1080" => json!(["P2160", "P1440", "P1080"]),
                "P2160" => json!(["P2160"]),
                _ => json!([]),
            };
            preferences.insert("requiredResolutions".to_string(), resolutions);
        }
        "debridStreamDolbyVisionFilter" => update_debrid_tag_filter(
            &mut preferences,
            value.as_str().unwrap_or("ANY"),
            &["DV", "DV_ONLY", "HDR_DV"],
        ),
        "debridStreamHdrFilter" => update_debrid_tag_filter(
            &mut preferences,
            value.as_str().unwrap_or("ANY"),
            &["HDR", "HDR10", "HDR10_PLUS", "HLG", "HDR_ONLY", "HDR_DV"],
        ),
        "debridStreamCodecFilter" => {
            let encodes = match value.as_str().unwrap_or("ANY") {
                "H264" => json!(["AVC"]),
                "HEVC" => json!(["HEVC"]),
                "AV1" => json!(["AV1"]),
                _ => json!([]),
            };
            preferences.insert("requiredEncodes".to_string(), encodes);
        }
        _ => {}
    }
    let encoded = serde_json::to_string(&Value::Object(preferences))?;
    set_typed_preference(
        blob,
        "debrid_settings",
        "debrid_stream_preferences",
        Kind::String,
        json!(encoded),
    )
}

fn update_debrid_tag_filter(preferences: &mut Map<String, Value>, mode: &str, tags: &[&str]) {
    let required = debrid_string_list(preferences.get("requiredVisualTags"));
    let excluded = debrid_string_list(preferences.get("excludedVisualTags"));
    let tag_set = tags
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut required = required
        .into_iter()
        .filter(|item| !tag_set.contains(item.as_str()))
        .collect::<Vec<_>>();
    let mut excluded = excluded
        .into_iter()
        .filter(|item| !tag_set.contains(item.as_str()))
        .collect::<Vec<_>>();
    match mode {
        "ONLY" => required.extend(tags.iter().map(|tag| (*tag).to_string())),
        "EXCLUDE" => excluded.extend(tags.iter().map(|tag| (*tag).to_string())),
        _ => {}
    }
    preferences.insert("requiredVisualTags".to_string(), json!(required));
    preferences.insert("excludedVisualTags".to_string(), json!(excluded));
}

fn debrid_string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
}

fn set_typed_preference(
    blob: &mut Value,
    feature: &str,
    key: &str,
    kind: Kind,
    value: Value,
) -> Result<()> {
    let root = blob
        .as_object_mut()
        .context("settings blob is not an object")?;
    let features = object_entry(root, "features")?;
    let feature_object = object_entry(features, feature)?;
    let type_name = match kind {
        Kind::Boolean => "boolean",
        Kind::String => "string",
        Kind::Int => "int",
        Kind::Float => "float",
    };
    feature_object.insert(
        key.to_string(),
        json!({ "type": type_name, "value": value }),
    );
    Ok(())
}

/// Removes credentials and local-only values that official Nuvio omits before
/// profile settings sync. The legacy aliases are from early versions of this
/// proof of concept and are scrubbed too, so one unrelated setting change
/// cannot put a secret or device-local preference back into the shared blob.
fn sanitize_profile_blob(blob: &mut Value) {
    const CREDENTIAL_KEYS: &[(&str, &[&str])] = &[
        (
            "player_settings",
            &[
                "animeskip_client_id",
                "introdb_api_key",
                "anime_skip_client_id",
                "intro_db_api_key",
                "intro_submit_enabled",
            ],
        ),
        (
            "debrid_settings",
            &[
                "debrid_torbox_api_key",
                "debrid_premiumize_api_key",
                "debrid_real_debrid_api_key",
            ],
        ),
        ("tmdb_settings", &["tmdb_api_key"]),
        ("mdblist_settings", &["mdblist_api_key"]),
    ];

    let Some(features) = blob.get_mut("features").and_then(Value::as_object_mut) else {
        return;
    };
    for (feature, keys) in CREDENTIAL_KEYS {
        let Some(settings) = features.get_mut(*feature).and_then(Value::as_object_mut) else {
            continue;
        };
        for key in *keys {
            settings.remove(*key);
        }
    }
}

fn object_entry<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    let entry = object.entry(key.to_string()).or_insert_with(|| json!({}));
    entry
        .as_object_mut()
        .with_context(|| format!("settings field {key} is not an object"))
}

fn typed_value<'a>(blob: &'a Value, feature: &str, key: &str, expected: &str) -> Option<&'a Value> {
    let value = blob.pointer(&format!("/features/{feature}/{key}"))?;
    (value.get("type")?.as_str()? == expected)
        .then(|| value.get("value"))
        .flatten()
}
fn typed_bool(blob: &Value, feature: &str, key: &str) -> Option<bool> {
    typed_value(blob, feature, key, "boolean")?.as_bool()
}
fn typed_string(blob: &Value, feature: &str, key: &str) -> Option<String> {
    typed_value(blob, feature, key, "string")?
        .as_str()
        .map(str::to_string)
}
fn typed_i64(blob: &Value, feature: &str, key: &str) -> Option<i64> {
    typed_value(blob, feature, key, "int")?.as_i64()
}
fn player_string(blob: &Value, key: &str, fallback: &str) -> String {
    typed_string(blob, "player_settings", key).unwrap_or_else(|| fallback.to_string())
}
fn player_string_set(blob: &Value, key: &str) -> Vec<String> {
    typed_value(blob, "player_settings", key, "string_set")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
fn player_bool(blob: &Value, key: &str, fallback: bool) -> bool {
    typed_bool(blob, "player_settings", key).unwrap_or(fallback)
}
fn player_int(blob: &Value, key: &str, fallback: i64) -> i64 {
    typed_i64(blob, "player_settings", key).unwrap_or(fallback)
}
fn typed_f64(blob: &Value, feature: &str, key: &str) -> Option<f64> {
    typed_value(blob, feature, key, "float")?.as_f64()
}
fn empty_blob() -> Value {
    json!({ "version": 3, "features": {} })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_player_settings_round_trip_and_validate() {
        let mut blob = json!({ "version": 3, "features": {} });
        for (key, value) in [
            ("subtitleOutlineWidth", json!(3)),
            ("autoplayTimeoutSeconds", json!(5)),
            ("holdToSpeedValue", json!(2.5)),
            ("subtitleTextColor", json!("#FFEEDDCC")),
            ("autoplaySource", json!("INSTALLED_ADDONS_ONLY")),
        ] {
            let (feature, storage_key, kind) = setting_path(key).expect(key);
            let normalized = validate_value(key, value, kind).expect(key);
            set_typed_preference(&mut blob, feature, storage_key, kind, normalized).unwrap();
        }
        blob.pointer_mut("/features/player_settings")
            .and_then(Value::as_object_mut)
            .unwrap()
            .insert(
                "stream_auto_play_selected_addons".to_string(),
                json!({ "type": "string_set", "value": ["Torrentio", "AIOStreams"] }),
            );
        let snapshot = snapshot(&blob);
        assert_eq!(snapshot.subtitle_outline_width, 3);
        assert_eq!(snapshot.autoplay_timeout_seconds, 5);
        assert_eq!(snapshot.hold_to_speed_value, 2.5);
        assert_eq!(snapshot.subtitle_text_color, "#FFEEDDCC");
        assert_eq!(snapshot.autoplay_source, "INSTALLED_ADDONS_ONLY");
        assert_eq!(
            snapshot.autoplay_selected_addons,
            vec!["Torrentio".to_string(), "AIOStreams".to_string()]
        );
        // Defaults must match Nuvio's, not zero.
        assert!(snapshot.hold_to_speed);
        assert_eq!(snapshot.subtitle_bottom_offset, 20);
        assert_eq!(snapshot.addon_subtitle_startup_mode, "ALL_SUBTITLES");
    }

    #[test]
    fn player_settings_reject_out_of_range_values() {
        for (key, value) in [
            ("subtitleOutlineWidth", json!(99)),
            ("autoplayTimeoutSeconds", json!(-1)),
            ("holdToSpeedValue", json!(10.0)),
            ("subtitleTextColor", json!("red")),
            ("autoplaySource", json!("WHATEVER")),
        ] {
            let (_, _, kind) = setting_path(key).expect(key);
            assert!(
                validate_value(key, value, kind).is_err(),
                "{key} should be rejected"
            );
        }
    }

    #[test]
    fn poster_writes_preserve_hover_preview_settings() {
        // The hover-preview and trailer options live in the same payload string.
        // Rewriting it from a partial view would silently reset them.
        let existing = json!({
            "widthDp": 134,
            "heightDp": 201,
            "cornerRadiusDp": 4,
            "hideLabelsEnabled": true,
            "hoverPreviewEnabled": true,
            "hoverPreviewOpenDelayMillis": 500,
            "hoverPreviewTrailerEnabled": true,
            "hoverPreviewTrailerSoundEnabled": true,
            "hoverPreviewTrailerStartSeconds": 3
        });
        let mut blob = json!({
            "version": 3,
            "features": { POSTER_PAYLOAD_KEY: existing.to_string() }
        });

        set_poster_style(&mut blob, "posterWidth", json!(104)).unwrap();

        let style = poster_style(&blob);
        assert_eq!(style["widthDp"], json!(104));
        // Height is derived, never left stale.
        assert_eq!(style["heightDp"], json!(156));
        assert_eq!(style["hoverPreviewTrailerEnabled"], json!(true));
        assert_eq!(style["hoverPreviewTrailerSoundEnabled"], json!(true));
        assert_eq!(style["hoverPreviewTrailerStartSeconds"], json!(3));
        assert_eq!(style["hoverPreviewOpenDelayMillis"], json!(500));
        assert_eq!(style["hideLabelsEnabled"], json!(true));
        assert_eq!(style["cornerRadiusDp"], json!(4));
    }

    #[test]
    fn poster_payload_is_a_json_string_with_every_field() {
        let mut blob = json!({ "version": 3, "features": {} });
        set_poster_style(&mut blob, "posterCornerRadius", json!(16)).unwrap();

        // Nuvio decodes this field as a String, so it must not become an object.
        let raw = blob
            .pointer(&format!("/features/{POSTER_PAYLOAD_KEY}"))
            .unwrap();
        assert!(raw.is_string());

        let style = poster_style(&blob);
        assert_eq!(style["cornerRadiusDp"], json!(16));
        assert_eq!(style["widthDp"], json!(DEFAULT_POSTER_WIDTH));
        assert_eq!(style["heightDp"], json!(189));
        // encodeDefaults = true on the Kotlin side, so defaults are materialised.
        assert_eq!(style["hoverPreviewEnabled"], json!(true));
        assert_eq!(style.as_object().unwrap().len(), 10);
    }

    #[test]
    fn poster_values_outside_the_supported_range_are_rejected() {
        let mut blob = json!({ "version": 3, "features": {} });
        assert!(set_poster_style(&mut blob, "posterWidth", json!(9000)).is_err());
        assert!(set_poster_style(&mut blob, "posterCornerRadius", json!(-1)).is_err());
        assert!(set_poster_style(&mut blob, "posterWidth", json!("wide")).is_err());
    }

    #[test]
    fn continue_watching_defaults_match_official_nuvio() {
        let settings = snapshot(&empty_blob());
        assert!(settings.continue_watching_visible);
        assert_eq!(settings.continue_watching_style, "Card");
        assert!(settings.continue_watching_up_next_from_furthest_episode);
        assert!(settings.continue_watching_use_episode_thumbnails);
        assert!(settings.continue_watching_show_unaired_next_up);
        assert!(!settings.continue_watching_blur_next_up);
        assert!(settings.dismissed_next_up.is_empty());
        assert!(settings.continue_watching_show_resume_prompt_on_launch);
        assert_eq!(settings.continue_watching_sort_mode, "DEFAULT");
    }

    #[test]
    fn continue_watching_writes_preserve_dismissals_and_unknown_fields() {
        let existing = json!({
            "isVisible": true,
            "style": "Card",
            "dismissedNextUpKeys": [" show|1|2 ", "show|1|2", "other|3|4"],
            "futureOption": { "keep": true }
        });
        let mut blob = json!({
            "version": 3,
            "features": {
                CONTINUE_WATCHING_PAYLOAD_KEY: existing.to_string(),
                "future_feature": { "keep": true }
            }
        });

        set_continue_watching_setting(&mut blob, "continueWatchingStyle", json!("Poster")).unwrap();

        let raw = blob
            .pointer(&format!("/features/{CONTINUE_WATCHING_PAYLOAD_KEY}"))
            .unwrap();
        assert!(raw.is_string());
        let payload: Value = serde_json::from_str(raw.as_str().unwrap()).unwrap();
        assert_eq!(payload["style"], json!("Poster"));
        assert_eq!(
            payload["dismissedNextUpKeys"],
            json!(["show|1|2", "other|3|4"])
        );
        assert_eq!(payload.pointer("/futureOption/keep"), Some(&json!(true)));
        assert_eq!(
            blob.pointer("/features/future_feature/keep"),
            Some(&json!(true))
        );
        // Kotlin's encodeDefaults=true payload shape is materialised on write.
        for key in [
            "isVisible",
            "style",
            "upNextFromFurthestEpisode",
            "use_episode_thumbnails_in_cw",
            "show_unaired_next_up",
            "blur_continue_watching_next_up",
            "dismissedNextUpKeys",
            "showResumePromptOnLaunch",
            "sort_mode",
        ] {
            assert!(payload.get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn continue_watching_rejects_invalid_types_and_enums() {
        let mut blob = empty_blob();
        assert!(
            set_continue_watching_setting(&mut blob, "continueWatchingVisible", json!("yes"))
                .is_err()
        );
        assert!(
            set_continue_watching_setting(&mut blob, "continueWatchingStyle", json!("Landscape"))
                .is_err()
        );
        assert!(
            set_continue_watching_setting(&mut blob, "continueWatchingSortMode", json!("RANDOM"))
                .is_err()
        );
    }

    #[test]
    fn typed_settings_preserve_unrelated_fields() {
        let mut blob = json!({ "version": 3, "features": { "future_feature": { "keep": true } } });
        set_typed_preference(
            &mut blob,
            "player_settings",
            "resize_mode",
            Kind::String,
            json!("Fill"),
        )
        .unwrap();
        assert_eq!(
            blob.pointer("/features/future_feature/keep"),
            Some(&json!(true))
        );
        assert_eq!(
            typed_string(&blob, "player_settings", "resize_mode").as_deref(),
            Some("Fill")
        );
        assert_eq!(snapshot(&blob).resize_mode, "Zoom");
    }

    #[test]
    fn provider_credential_merge_preserves_unknown_rows_and_fields() {
        let current = ProviderCredentialStore::from_rpc_value(&json!([
            {
                "provider": "tmdb",
                "credential_json": {
                    "api_key": "old",
                    "future_field": { "keep": true }
                },
                "updated_at": "ignored by the push contract"
            },
            {
                "provider": "debrid:future-service",
                "credential_json": {
                    "api_key": "secret",
                    "region": "west"
                }
            },
            {
                "provider": "animeskip",
                "credential_json": {
                    "client_id": "anime-client",
                    "future_field": "keep"
                }
            },
            {
                "provider": "introdb",
                "credential_json": { "api_key": "intro-key" }
            }
        ]))
        .unwrap();

        let next = current.with_credential("tmdb", "  replacement  ").unwrap();
        let params = next.push_params(7, "desktop-client");

        assert_eq!(params["p_profile_id"], json!(7));
        assert_eq!(params["p_origin_client_id"], json!("desktop-client"));
        assert_eq!(
            params.pointer("/p_credentials/0/credential_json/api_key"),
            Some(&json!("replacement"))
        );
        assert_eq!(
            params.pointer("/p_credentials/0/credential_json/future_field/keep"),
            Some(&json!(true))
        );
        assert_eq!(
            params.pointer("/p_credentials/1/provider"),
            Some(&json!("debrid:future-service"))
        );
        assert_eq!(
            params.pointer("/p_credentials/1/credential_json/region"),
            Some(&json!("west"))
        );
        let anime = next
            .with_credential("animeskip", " replacement-client ")
            .unwrap();
        assert_eq!(anime.anime_skip_client_id(), "replacement-client");
        assert_eq!(anime.intro_db_api_key(), "intro-key");
        let snapshot = anime.snapshot();
        assert_eq!(snapshot.anime_skip_client_id, "replacement-client");
        assert_eq!(snapshot.intro_db_api_key, "intro-key");
        let anime_params = anime.push_params(7, "desktop-client");
        assert_eq!(
            anime_params.pointer("/p_credentials/2/credential_json/client_id"),
            Some(&json!("replacement-client"))
        );
        assert_eq!(
            anime_params.pointer("/p_credentials/2/credential_json/future_field"),
            Some(&json!("keep"))
        );
    }

    #[test]
    fn provider_seed_uses_official_fields_without_a_replace_payload() {
        let params = required_provider_seed_params(4, "desktop-client");
        assert_eq!(params["p_profile_id"], json!(4));
        assert_eq!(params["p_origin_client_id"], json!("desktop-client"));
        assert_eq!(
            params.pointer("/p_credentials/0/credential_json/api_key"),
            Some(&json!(""))
        );
        assert_eq!(
            params.pointer("/p_credentials/2/credential_json/client_id"),
            Some(&json!(""))
        );
        assert_eq!(
            params.pointer("/p_credentials/3/provider"),
            Some(&json!("introdb"))
        );
        assert_eq!(
            params.pointer("/p_credentials/4/provider"),
            Some(&json!("debrid:torbox"))
        );
        assert_eq!(
            params.pointer("/p_credentials/5/provider"),
            Some(&json!("debrid:premiumize"))
        );
        assert_eq!(
            params.pointer("/p_credentials/6/provider"),
            Some(&json!("debrid:realdebrid"))
        );
    }

    #[test]
    fn profile_blob_policy_strips_secrets_and_local_only_values() {
        let mut blob = json!({
            "version": 3,
            "features": {
                "player_settings": {
                    "animeskip_client_id": { "type": "string", "value": "secret" },
                    "introdb_api_key": { "type": "string", "value": "secret" },
                    "anime_skip_client_id": { "type": "string", "value": "legacy" },
                    "intro_db_api_key": { "type": "string", "value": "legacy" },
                    "intro_submit_enabled": { "type": "boolean", "value": true },
                    "animeskip_enabled": { "type": "boolean", "value": true },
                    "future_player_option": { "keep": true }
                },
                "tmdb_settings": {
                    "tmdb_api_key": { "type": "string", "value": "secret" },
                    "tmdb_enabled": { "type": "boolean", "value": true }
                },
                "mdblist_settings": {
                    "mdblist_api_key": { "type": "string", "value": "secret" },
                    "mdblist_use_imdb": { "type": "boolean", "value": false }
                },
                "debrid_settings": {
                    "debrid_torbox_api_key": { "type": "string", "value": "secret" },
                    "debrid_premiumize_api_key": { "type": "string", "value": "secret" },
                    "debrid_real_debrid_api_key": { "type": "string", "value": "secret" },
                    "future_debrid_option": "keep"
                },
                "future_feature": { "keep": true }
            }
        });

        sanitize_profile_blob(&mut blob);

        for pointer in [
            "/features/player_settings/animeskip_client_id",
            "/features/player_settings/introdb_api_key",
            "/features/player_settings/anime_skip_client_id",
            "/features/player_settings/intro_db_api_key",
            "/features/player_settings/intro_submit_enabled",
            "/features/tmdb_settings/tmdb_api_key",
            "/features/mdblist_settings/mdblist_api_key",
            "/features/debrid_settings/debrid_torbox_api_key",
            "/features/debrid_settings/debrid_premiumize_api_key",
            "/features/debrid_settings/debrid_real_debrid_api_key",
        ] {
            assert!(
                blob.pointer(pointer).is_none(),
                "{pointer} should be stripped"
            );
        }
        assert_eq!(
            blob.pointer("/features/player_settings/animeskip_enabled/value"),
            Some(&json!(true))
        );
        assert_eq!(
            blob.pointer("/features/player_settings/future_player_option/keep"),
            Some(&json!(true))
        );
        assert_eq!(
            blob.pointer("/features/future_feature/keep"),
            Some(&json!(true))
        );
    }

    #[test]
    fn integration_settings_use_official_keys_and_defaults() {
        let mut blob = empty_blob();
        let defaults = snapshot(&blob);
        assert!(!defaults.tmdb_enabled);
        assert_eq!(defaults.tmdb_language, "en");
        assert!(defaults.tmdb_use_trailers);
        assert!(!defaults.tmdb_use_release_dates);
        assert!(defaults.tmdb_use_collections);
        assert!(!defaults.mdb_list_enabled);
        assert!(defaults.mdb_list_use_imdb);
        assert!(defaults.mdb_list_use_mal);
        assert!(!defaults.debrid_enabled);
        assert!(defaults.debrid_cloud_library_enabled);
        assert_eq!(defaults.debrid_preferred_resolver_provider_id, "");
        assert_eq!(defaults.debrid_instant_playback_preparation_limit, 0);
        assert_eq!(defaults.debrid_stream_max_results, 0);
        assert_eq!(defaults.debrid_stream_sort_mode, "DEFAULT");
        assert_eq!(defaults.debrid_stream_minimum_quality, "ANY");
        assert_eq!(defaults.debrid_stream_dolby_vision_filter, "ANY");
        assert_eq!(defaults.debrid_stream_hdr_filter, "ANY");
        assert_eq!(defaults.debrid_stream_codec_filter, "ANY");
        assert_eq!(
            defaults.debrid_stream_name_template,
            DEFAULT_DEBRID_STREAM_NAME_TEMPLATE
        );

        for (key, value) in [
            ("tmdbEnabled", json!(true)),
            ("tmdbLanguage", json!("pt_BR")),
            ("tmdbUseReleaseDates", json!(true)),
            ("mdbListEnabled", json!(true)),
            ("mdbListUseImdb", json!(false)),
        ] {
            let (feature, storage_key, kind) = setting_path(key).unwrap();
            let value = validate_value(key, value, kind).unwrap();
            set_typed_preference(&mut blob, feature, storage_key, kind, value).unwrap();
        }

        assert_eq!(
            blob.pointer("/features/tmdb_settings/tmdb_language/value"),
            Some(&json!("pt-BR"))
        );
        assert_eq!(
            blob.pointer("/features/mdblist_settings/mdblist_use_imdb/value"),
            Some(&json!(false))
        );
        let saved = snapshot(&blob);
        assert!(saved.tmdb_enabled);
        assert_eq!(saved.tmdb_language, "pt-BR");
        assert!(saved.tmdb_use_release_dates);
        assert!(saved.mdb_list_enabled);
        assert!(!saved.mdb_list_use_imdb);

        let serialized = serde_json::to_string(&saved).unwrap();
        assert!(!serialized.contains("apiKey"));
        assert!(!serialized.contains("api_key"));
    }

    #[test]
    fn debrid_credentials_use_exact_rows_and_hide_real_debrid_from_resolver_policy() {
        let current = ProviderCredentialStore::from_rpc_value(&json!([
            {
                "provider": "debrid:premiumize",
                "credential_json": { "api_key": "pm", "future": true }
            },
            {
                "provider": "debrid:realdebrid",
                "credential_json": { "api_key": "rd" }
            }
        ]))
        .unwrap();
        assert_eq!(
            current.configured_debrid_resolver_provider_ids(),
            vec!["premiumize"]
        );

        let next = current
            .with_credential("debrid:torbox", "  torbox-token  ")
            .unwrap();
        let snapshot = next.snapshot();
        assert_eq!(snapshot.torbox_api_key, "torbox-token");
        assert_eq!(snapshot.premiumize_api_key, "pm");
        assert_eq!(snapshot.real_debrid_api_key, "rd");
        assert_eq!(
            next.configured_debrid_resolver_provider_ids(),
            vec!["torbox", "premiumize"]
        );
        let params = next.push_params(3, "desktop-client");
        assert_eq!(
            params.pointer("/p_credentials/0/credential_json/future"),
            Some(&json!(true))
        );
    }

    #[test]
    fn debrid_legacy_controls_update_json_preferences_without_erasing_unknowns() {
        let mut blob = empty_blob();
        let initial = json!({
            "futureRule": { "keep": true },
            "requiredVisualTags": ["AI", "DV"],
            "excludedVisualTags": ["THREE_D"]
        })
        .to_string();
        set_typed_preference(
            &mut blob,
            "debrid_settings",
            "debrid_stream_preferences",
            Kind::String,
            json!(initial),
        )
        .unwrap();

        sync_legacy_debrid_stream_preference(
            &mut blob,
            "debridStreamDolbyVisionFilter",
            &json!("EXCLUDE"),
        )
        .unwrap();
        sync_legacy_debrid_stream_preference(&mut blob, "debridStreamSortMode", &json!("SIZE_ASC"))
            .unwrap();

        let encoded = typed_string(&blob, "debrid_settings", "debrid_stream_preferences").unwrap();
        let preferences: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(preferences.pointer("/futureRule/keep"), Some(&json!(true)));
        assert_eq!(
            preferences.pointer("/requiredVisualTags"),
            Some(&json!(["AI"]))
        );
        assert_eq!(
            preferences.pointer("/excludedVisualTags"),
            Some(&json!(["THREE_D", "DV", "DV_ONLY", "HDR_DV"]))
        );
        assert_eq!(
            preferences.pointer("/sortCriteria/0"),
            Some(&json!({ "key": "SIZE", "direction": "ASC" }))
        );
    }
}

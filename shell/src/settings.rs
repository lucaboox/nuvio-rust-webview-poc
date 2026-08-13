use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::auth::AuthService;
use crate::metadata::{MdbListMetadataSettings, MetadataConfig, TmdbMetadataSettings};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub amoled_enabled: bool,
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
    pub autoplay_regex: String,
    pub autoplay_timeout_seconds: i64,
    pub autoplay_prefer_binge_group: bool,
    pub autoplay_reuse_binge_group: bool,
    pub autoplay_next_episode_fallback: bool,
    // Skip segments
    pub anime_skip_enabled: bool,
    pub anime_skip_client_id: String,
    pub intro_db_api_key: String,
    pub intro_submit_enabled: bool,
    // Gestures (Nuvio syncs these; they are not client-only)
    pub hold_to_speed: bool,
    pub hold_to_speed_value: f64,
    // External player
    pub external_player_enabled: bool,
    pub external_player_id: String,
    pub external_player_forward_subtitles: bool,
    pub external_player_send_skip_segments: bool,
}

/// Nuvio stores poster card style as a JSON *string* inside the settings blob
/// rather than as typed preference entries, so it needs its own read/write path.
const POSTER_PAYLOAD_KEY: &str = "poster_card_style_settings_payload";
const DEFAULT_POSTER_WIDTH: i64 = 126;
const DEFAULT_POSTER_CORNER_RADIUS: i64 = 12;

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
    let read_i64 = |key: &str, fallback: i64| style.get(key).and_then(Value::as_i64).unwrap_or(fallback);
    let read_bool = |key: &str, fallback: bool| {
        style.get(key).and_then(Value::as_bool).unwrap_or(fallback)
    };
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
            let width = value.as_i64().context("poster width must be a whole number")?;
            bail_unless(
                (80..=240).contains(&width),
                "poster width must be between 80 and 240",
            )?;
            style.insert("widthDp".to_string(), json!(width));
            style.insert("heightDp".to_string(), json!(width * 3 / 2));
        }
        "posterCornerRadius" => {
            let radius = value.as_i64().context("corner radius must be a whole number")?;
            bail_unless(
                (0..=32).contains(&radius),
                "corner radius must be between 0 and 32",
            )?;
            style.insert("cornerRadiusDp".to_string(), json!(radius));
        }
        "posterHideLabels" => {
            style.insert(
                "hideLabelsEnabled".to_string(),
                json!(value.as_bool().context("hide labels must be true or false")?),
            );
        }
        "posterLandscapeCatalogs" => {
            style.insert(
                "catalogLandscapeModeEnabled".to_string(),
                json!(value
                    .as_bool()
                    .context("landscape posters must be true or false")?),
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
    let blob = response
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("settings_json"))
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(empty_blob);
    Ok((snapshot(&blob), blob))
}

pub fn load_metadata_config(auth: &AuthService, profile_id: i32) -> Result<MetadataConfig> {
    let (_, blob) = load(auth, profile_id)?;
    let credentials = auth
        .rpc_value(
            "sync_pull_provider_credentials",
            &json!({ "p_profile_id": profile_id }),
        )
        .unwrap_or_else(|_| json!([]));
    let credential = |provider: &str| {
        credentials
            .as_array()
            .and_then(|rows| {
                rows.iter().find(|row| {
                    row.get("provider")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.eq_ignore_ascii_case(provider))
                })
            })
            .and_then(|row| row.pointer("/credential_json/api_key"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let tmdb_api_key = credential("tmdb");
    let mdblist_api_key = credential("mdblist");
    Ok(MetadataConfig {
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
            use_episodes: typed_bool(&blob, "tmdb_settings", "tmdb_use_episodes").unwrap_or(true),
            use_season_posters: typed_bool(&blob, "tmdb_settings", "tmdb_use_season_posters")
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
    })
}

pub fn update(
    auth: &AuthService,
    profile_id: i32,
    key: &str,
    value: Value,
) -> Result<SettingsSnapshot> {
    let (_, mut blob) = load(auth, profile_id)?;
    if key.starts_with("poster") {
        set_poster_style(&mut blob, key, value)?;
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
        set_typed_preference(&mut blob, feature, storage_key, kind, normalized)?;
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
    Ok(snapshot(&blob))
}

fn snapshot(blob: &Value) -> SettingsSnapshot {
    SettingsSnapshot {
        amoled_enabled: typed_bool(blob, "theme_settings", "amoled_enabled").unwrap_or(false),
        show_loading_overlay: typed_bool(blob, "player_settings", "show_loading_overlay")
            .unwrap_or(true),
        show_parental_guide: typed_bool(blob, "player_settings", "show_parental_guide")
            .unwrap_or(true),
        resize_mode: typed_string(blob, "player_settings", "resize_mode")
            .unwrap_or_else(|| "Fit".to_string()),
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
        autoplay_regex: player_string(blob, "stream_auto_play_regex", ""),
        autoplay_timeout_seconds: player_int(blob, "stream_auto_play_timeout_seconds", 3),
        autoplay_prefer_binge_group: player_bool(
            blob,
            "stream_auto_play_prefer_binge_group",
            true,
        ),
        autoplay_reuse_binge_group: player_bool(blob, "stream_auto_play_reuse_binge_group", true),
        autoplay_next_episode_fallback: player_bool(
            blob,
            "stream_auto_play_next_episode_fallback_enabled",
            true,
        ),
        anime_skip_enabled: player_bool(blob, "anime_skip_enabled", false),
        anime_skip_client_id: player_string(blob, "anime_skip_client_id", ""),
        intro_db_api_key: player_string(blob, "intro_db_api_key", ""),
        intro_submit_enabled: player_bool(blob, "intro_submit_enabled", false),
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
        "subtitleBackgroundColor" => (
            "player_settings",
            "subtitle_background_color",
            Kind::String,
        ),
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
        "animeSkipEnabled" => ("player_settings", "anime_skip_enabled", Kind::Boolean),
        "animeSkipClientId" => ("player_settings", "anime_skip_client_id", Kind::String),
        "introDbApiKey" => ("player_settings", "intro_db_api_key", Kind::String),
        "introSubmitEnabled" => ("player_settings", "intro_submit_enabled", Kind::Boolean),
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
        "subtitleFontSize" if !(12..=40).contains(&value.as_i64().unwrap_or_default()) => {
            bail!("subtitle size must be between 12 and 40")
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
                Some("ALL_SOURCES" | "ADDONS_ONLY" | "PLUGINS_ONLY" | "SELECTED")
            ) =>
        {
            bail!("unsupported autoplay source")
        }
        "addonSubtitleStartupMode"
            if !matches!(
                value.as_str(),
                Some("ALL_SUBTITLES" | "PREFERRED_ONLY" | "NONE")
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
        "preferredAudioLanguage" | "preferredSubtitleLanguage"
            if value.as_str().is_some_and(|text| text.len() > 16) =>
        {
            bail!("invalid language code")
        }
        _ => {}
    }
    Ok(value)
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
            ("autoplaySource", json!("ADDONS_ONLY")),
        ] {
            let (feature, storage_key, kind) = setting_path(key).expect(key);
            let normalized = validate_value(key, value, kind).expect(key);
            set_typed_preference(&mut blob, feature, storage_key, kind, normalized).unwrap();
        }
        let snapshot = snapshot(&blob);
        assert_eq!(snapshot.subtitle_outline_width, 3);
        assert_eq!(snapshot.autoplay_timeout_seconds, 5);
        assert_eq!(snapshot.hold_to_speed_value, 2.5);
        assert_eq!(snapshot.subtitle_text_color, "#FFEEDDCC");
        assert_eq!(snapshot.autoplay_source, "ADDONS_ONLY");
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
            assert!(validate_value(key, value, kind).is_err(), "{key} should be rejected");
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
        let raw = blob.pointer(&format!("/features/{POSTER_PAYLOAD_KEY}")).unwrap();
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
    }
}

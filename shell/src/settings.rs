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
    pub autoplay_mode: String,
    pub autoplay_next_episode: bool,
    pub skip_intro: bool,
    pub rtx_super_resolution: bool,
    pub show_file_size_badges: bool,
    pub badge_placement: String,
    pub episode_release_alerts: bool,
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
    if key == "episodeReleaseAlerts" {
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
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Boolean,
    String,
    Int,
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
fn empty_blob() -> Value {
    json!({ "version": 3, "features": {} })
}

#[cfg(test)]
mod tests {
    use super::*;

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

use anyhow::{Context, Result};
use rayon::prelude::*;
use reqwest::blocking::Client;
use serde_json::{Value, json};
use url::Url;

use crate::content::{ContentMeta, ExternalRating, MetaPerson, MetaTrailer};

#[derive(Clone, Debug, Default)]
pub struct MetadataConfig {
    pub tmdb: TmdbMetadataSettings,
    pub mdblist: MdbListMetadataSettings,
}

#[derive(Clone, Debug)]
pub struct TmdbMetadataSettings {
    pub enabled: bool,
    pub api_key: String,
    pub language: String,
    pub use_trailers: bool,
    pub use_artwork: bool,
    pub use_basic_info: bool,
    pub use_details: bool,
    pub use_release_dates: bool,
    pub use_credits: bool,
    pub use_episodes: bool,
    pub use_season_posters: bool,
}

impl Default for TmdbMetadataSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            language: "en".to_string(),
            use_trailers: true,
            use_artwork: true,
            use_basic_info: true,
            use_details: true,
            use_release_dates: false,
            use_credits: true,
            use_episodes: true,
            use_season_posters: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MdbListMetadataSettings {
    pub enabled: bool,
    pub api_key: String,
    pub providers: Vec<String>,
}

pub fn enrich_tmdb(client: &Client, mut meta: ContentMeta, config: &MetadataConfig) -> ContentMeta {
    if config.tmdb.enabled
        && let Ok(Some(tmdb_id)) = resolve_tmdb_id(client, &meta, &config.tmdb)
        && let Ok(payload) = fetch_tmdb_details(client, &meta.content_type, tmdb_id, &config.tmdb)
    {
        apply_tmdb(client, &mut meta, tmdb_id, &payload, &config.tmdb);
    }
    meta
}

pub fn enrich_ratings(
    client: &Client,
    mut meta: ContentMeta,
    config: &MetadataConfig,
) -> ContentMeta {
    if config.mdblist.enabled {
        meta.external_ratings = fetch_mdblist_ratings(client, &meta, &config.mdblist);
    }
    meta
}

pub fn addon_lookup_id(
    client: &Client,
    content_type: &str,
    id: &str,
    settings: &TmdbMetadataSettings,
) -> String {
    let Some(tmdb_id) = id
        .strip_prefix("tmdb:")
        .and_then(|value| value.split([':', '/']).next())
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return id.to_string();
    };
    if settings.api_key.is_empty() {
        return id.to_string();
    }
    let media = if is_series(content_type) {
        "tv"
    } else {
        "movie"
    };
    tmdb_get(
        client,
        &format!("{media}/{tmdb_id}/external_ids"),
        settings,
        &[],
    )
    .ok()
    .and_then(|value| string(&value, "imdb_id"))
    .unwrap_or_else(|| id.to_string())
}

pub fn standalone(
    client: &Client,
    content_type: &str,
    id: &str,
    config: &MetadataConfig,
) -> Result<ContentMeta> {
    if config.tmdb.api_key.is_empty() {
        anyhow::bail!("no installed addon resolved this title and TMDB is not configured");
    }
    let seed = ContentMeta {
        id: id.to_string(),
        content_type: content_type.to_string(),
        name: id.to_string(),
        ..Default::default()
    };
    let tmdb_id = resolve_tmdb_id(client, &seed, &config.tmdb)?
        .context("TMDB could not identify this title")?;
    let payload = fetch_tmdb_details(client, content_type, tmdb_id, &config.tmdb)?;
    let mut settings = config.tmdb.clone();
    settings.enabled = true;
    let mut result = seed;
    apply_tmdb(client, &mut result, tmdb_id, &payload, &settings);
    Ok(result)
}

fn resolve_tmdb_id(
    client: &Client,
    meta: &ContentMeta,
    settings: &TmdbMetadataSettings,
) -> Result<Option<i64>> {
    let raw = meta.id.trim();
    if let Some(value) = raw
        .strip_prefix("tmdb:")
        .and_then(|value| value.split([':', '/']).next())
        .and_then(|value| value.parse().ok())
    {
        return Ok(Some(value));
    }
    let normalized = raw
        .strip_prefix("movie:")
        .or_else(|| raw.strip_prefix("series:"))
        .unwrap_or(raw)
        .split([':', '/'])
        .next()
        .unwrap_or_default();
    if let Ok(value) = normalized.parse() {
        return Ok(Some(value));
    }
    if !normalized.to_ascii_lowercase().starts_with("tt") {
        return Ok(None);
    }
    let payload = tmdb_get(
        client,
        &format!("find/{normalized}"),
        settings,
        &[("external_source", "imdb_id")],
    )?;
    let key = if is_series(&meta.content_type) {
        "tv_results"
    } else {
        "movie_results"
    };
    Ok(payload
        .get(key)
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("id"))
        .and_then(Value::as_i64))
}

fn fetch_tmdb_details(
    client: &Client,
    content_type: &str,
    tmdb_id: i64,
    settings: &TmdbMetadataSettings,
) -> Result<Value> {
    let media = if is_series(content_type) {
        "tv"
    } else {
        "movie"
    };
    // Keep artwork retrieval aligned with the Kotlin client. TMDB can return a
    // different logo ordering when images are appended to the details request,
    // while Nuvio reads the dedicated /images response.
    let append = if media == "tv" {
        "credits,videos,content_ratings"
    } else {
        "credits,videos,release_dates"
    };
    let language = normalize_tmdb_language(&settings.language);
    let language_code = language.split('-').next().unwrap_or("en");
    let include_image_language = format!("{language_code},{language},en,null");
    let details_path = format!("{media}/{tmdb_id}");
    let images_path = format!("{media}/{tmdb_id}/images");
    let (details, images) = rayon::join(
        || {
            tmdb_get(
                client,
                &details_path,
                settings,
                &[("append_to_response", append)],
            )
        },
        || {
            tmdb_get(
                client,
                &images_path,
                settings,
                &[("include_image_language", &include_image_language)],
            )
        },
    );
    let mut details = details?;
    if let (Some(details), Ok(images)) = (details.as_object_mut(), images) {
        details.insert("images".to_string(), images);
    }
    Ok(details)
}

fn apply_tmdb(
    client: &Client,
    meta: &mut ContentMeta,
    tmdb_id: i64,
    value: &Value,
    settings: &TmdbMetadataSettings,
) {
    if settings.use_artwork {
        meta.background = image(value, "backdrop_path", "w1280").or(meta.background.take());
        meta.poster = image(value, "poster_path", "w500").or(meta.poster.take());
        meta.logo = value
            .pointer("/images/logos")
            .and_then(Value::as_array)
            .and_then(|items| select_localized_logo(items, &settings.language))
            .and_then(|item| image(item, "file_path", "w500"))
            .or(meta.logo.take());
    }
    if settings.use_basic_info {
        meta.name = string(
            value,
            if is_series(&meta.content_type) {
                "name"
            } else {
                "title"
            },
        )
        .unwrap_or_else(|| meta.name.clone());
        meta.description = string(value, "overview").or(meta.description.take());
        let genres = value
            .get("genres")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| string(item, "name"))
            .collect::<Vec<_>>();
        if !genres.is_empty() {
            meta.genres = genres;
        }
        if meta.imdb_rating.as_deref().is_none_or(str::is_empty) {
            meta.imdb_rating = value
                .get("vote_average")
                .and_then(Value::as_f64)
                .map(|rating| format!("{rating:.1}"));
        }
    }
    if settings.use_details {
        meta.status = string(value, "status").or(meta.status.take());
        let runtime = value
            .get("runtime")
            .and_then(Value::as_i64)
            .or_else(|| value.pointer("/episode_run_time/0").and_then(Value::as_i64));
        meta.runtime = runtime
            .map(|minutes| format!("{minutes}m"))
            .or(meta.runtime.take());
        meta.language = string(value, "original_language").or(meta.language.take());
        let countries = value
            .get("production_countries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| string(item, "iso_3166_1"))
            .collect::<Vec<_>>();
        if !countries.is_empty() {
            meta.country = Some(countries.join(", "));
        }
        meta.age_rating =
            age_rating(value, is_series(&meta.content_type)).or(meta.age_rating.take());
    }
    if settings.use_release_dates {
        meta.release_info = string(
            value,
            if is_series(&meta.content_type) {
                "first_air_date"
            } else {
                "release_date"
            },
        )
        .or(meta.release_info.take());
        if is_series(&meta.content_type) {
            meta.last_air_date = string(value, "last_air_date").or(meta.last_air_date.take());
        }
    }
    if settings.use_credits {
        apply_credits(meta, value);
    }
    if settings.use_trailers {
        let trailers = parse_tmdb_trailers(value.pointer("/videos/results"));
        if !trailers.is_empty() {
            meta.trailers = trailers;
        }
    }
    if is_series(&meta.content_type)
        && (settings.use_episodes || settings.use_release_dates || settings.use_season_posters)
    {
        apply_episode_metadata(client, meta, tmdb_id, settings);
    }
}

fn apply_credits(meta: &mut ContentMeta, value: &Value) {
    let mut people = Vec::new();
    if is_series(&meta.content_type) {
        for creator in value
            .get("created_by")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(name) = string(creator, "name") {
                people.push(MetaPerson {
                    name,
                    role: Some("Creator".to_string()),
                    photo: image(creator, "profile_path", "w500"),
                    tmdb_id: creator.get("id").and_then(Value::as_i64),
                });
            }
        }
    }
    let crew = value.pointer("/credits/crew").and_then(Value::as_array);
    let director_job = if is_series(&meta.content_type) {
        "Creator"
    } else {
        "Director"
    };
    let mut directors = Vec::new();
    let mut writers = Vec::new();
    for member in crew.into_iter().flatten() {
        let job = string(member, "job").unwrap_or_default();
        let Some(name) = string(member, "name") else {
            continue;
        };
        if job.eq_ignore_ascii_case("Director") {
            directors.push(name.clone());
            if !is_series(&meta.content_type) {
                people.push(person(member, name, director_job));
            }
        } else if job.to_ascii_lowercase().contains("writer")
            || job.to_ascii_lowercase().contains("screenplay")
        {
            writers.push(name.clone());
            if directors.is_empty() && !is_series(&meta.content_type) {
                people.push(person(member, name, "Writer"));
            }
        }
    }
    for member in value
        .pointer("/credits/cast")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = string(member, "name") {
            people.push(MetaPerson {
                name,
                role: string(member, "character"),
                photo: image(member, "profile_path", "w500"),
                tmdb_id: member.get("id").and_then(Value::as_i64),
            });
        }
    }
    people.dedup_by(|left, right| {
        left.name.eq_ignore_ascii_case(&right.name) && left.role == right.role
    });
    if !people.is_empty() {
        meta.cast = people;
    }
    if is_series(&meta.content_type) {
        meta.director = value
            .get("created_by")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| string(item, "name"))
            .collect();
    } else if !directors.is_empty() {
        meta.director = directors;
    }
    if meta.director.is_empty() && !writers.is_empty() {
        meta.writer = writers;
    }
}

fn apply_episode_metadata(
    client: &Client,
    meta: &mut ContentMeta,
    tmdb_id: i64,
    settings: &TmdbMetadataSettings,
) {
    let seasons = meta
        .videos
        .iter()
        .filter_map(|video| video.season)
        .collect::<std::collections::BTreeSet<_>>();
    let results = seasons
        .par_iter()
        .filter_map(|season| {
            tmdb_get(
                client,
                &format!("tv/{tmdb_id}/season/{season}"),
                settings,
                &[],
            )
            .ok()
            .map(|value| (*season, value))
        })
        .collect::<Vec<_>>();
    for (season, value) in results {
        let season_poster = image(&value, "poster_path", "w500");
        for episode in value
            .get("episodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(number) = episode.get("episode_number").and_then(Value::as_i64) else {
                continue;
            };
            let Some(video) = meta
                .videos
                .iter_mut()
                .find(|video| video.season == Some(season) && video.episode == Some(number))
            else {
                continue;
            };
            if settings.use_episodes {
                video.title = string(episode, "name").unwrap_or_else(|| video.title.clone());
                video.overview = string(episode, "overview").or(video.overview.take());
                video.thumbnail = image(episode, "still_path", "w500").or(video.thumbnail.take());
                video.runtime = episode
                    .get("runtime")
                    .and_then(Value::as_i64)
                    .or(video.runtime);
            }
            if settings.use_release_dates {
                video.released = string(episode, "air_date").or(video.released.take());
            }
            if settings.use_season_posters {
                video.season_poster = season_poster.clone().or(video.season_poster.take());
            }
        }
    }
}

fn fetch_mdblist_ratings(
    client: &Client,
    meta: &ContentMeta,
    settings: &MdbListMetadataSettings,
) -> Vec<ExternalRating> {
    let Some(imdb) = extract_imdb_id(&meta.id) else {
        return Vec::new();
    };
    let media = if is_series(&meta.content_type) {
        "show"
    } else {
        "movie"
    };
    settings
        .providers
        .par_iter()
        .filter_map(|source| {
            let mut url =
                Url::parse(&format!("https://api.mdblist.com/rating/{media}/{source}")).ok()?;
            url.query_pairs_mut()
                .append_pair("apikey", &settings.api_key);
            let payload: Value = client
                .post(url)
                .json(&json!({ "ids": [imdb], "provider": "imdb" }))
                .send()
                .ok()?
                .error_for_status()
                .ok()?
                .json()
                .ok()?;
            payload
                .pointer("/ratings/0/rating")
                .and_then(Value::as_f64)
                .map(|value| ExternalRating {
                    source: source.clone(),
                    value,
                })
        })
        .collect()
}

fn tmdb_get(
    client: &Client,
    path: &str,
    settings: &TmdbMetadataSettings,
    extras: &[(&str, &str)],
) -> Result<Value> {
    let mut url = Url::parse(&format!("https://api.themoviedb.org/3/{path}"))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("api_key", &settings.api_key);
        query.append_pair("language", &settings.language);
        for (key, value) in extras {
            query.append_pair(key, value);
        }
    }
    client
        .get(url)
        .send()?
        .error_for_status()?
        .json()
        .context("TMDB returned invalid JSON")
}

fn person(value: &Value, name: String, role: &str) -> MetaPerson {
    MetaPerson {
        name,
        role: Some(role.to_string()),
        photo: image(value, "profile_path", "w500"),
        tmdb_id: value.get("id").and_then(Value::as_i64),
    }
}

fn parse_tmdb_trailers(value: Option<&Value>) -> Vec<MetaTrailer> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let key = string(item, "key")?;
            if !string(item, "site")
                .unwrap_or_default()
                .eq_ignore_ascii_case("youtube")
            {
                return None;
            }
            Some(MetaTrailer {
                id: string(item, "id").unwrap_or_else(|| key.clone()),
                key,
                name: string(item, "name").unwrap_or_else(|| "Trailer".to_string()),
                site: "YouTube".to_string(),
                trailer_type: string(item, "type").unwrap_or_else(|| "Trailer".to_string()),
                official: item.get("official").and_then(Value::as_bool),
                published_at: string(item, "published_at"),
                season_number: None,
            })
        })
        .collect()
}

fn age_rating(value: &Value, series: bool) -> Option<String> {
    let rows = if series {
        value.pointer("/content_ratings/results")
    } else {
        value.pointer("/release_dates/results")
    }?
    .as_array()?;
    let preferred = rows
        .iter()
        .find(|row| string(row, "iso_3166_1").as_deref() == Some("US"))
        .or_else(|| rows.first())?;
    if series {
        string(preferred, "rating")
    } else {
        preferred
            .get("release_dates")?
            .as_array()?
            .iter()
            .filter_map(|row| string(row, "certification"))
            .find(|value| !value.is_empty())
    }
}

fn extract_imdb_id(value: &str) -> Option<String> {
    let start = value.find("tt")?;
    let id = value[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    (id.len() > 2 && id[2..].chars().all(|ch| ch.is_ascii_digit())).then_some(id)
}

fn normalize_tmdb_language(value: &str) -> String {
    let raw = value.trim().replace('_', "-");
    if raw.is_empty() {
        return "en".to_string();
    }
    let mut parts = raw.splitn(2, '-');
    let language = parts.next().unwrap_or("en").to_ascii_lowercase();
    let normalized = parts
        .next()
        .filter(|region| !region.is_empty())
        .map(|region| format!("{language}-{}", region.to_ascii_uppercase()))
        .unwrap_or(language);
    if normalized == "es-419" {
        "es-MX".to_string()
    } else {
        normalized
    }
}

fn select_localized_logo<'a>(items: &'a [Value], language: &str) -> Option<&'a Value> {
    let normalized = normalize_tmdb_language(language);
    let language_code = normalized.split('-').next().unwrap_or("en");
    let explicit_region = normalized.split_once('-').map(|(_, region)| region);
    let default_region = match language_code {
        "pt" => Some("PT"),
        "es" => Some("ES"),
        _ => None,
    };
    let region = explicit_region.or(default_region);
    // Kotlin's sortedWith is stable, so the first TMDB result wins when two
    // logos have the same localization rank. Iterating in reverse preserves
    // that behavior with max_by_key (which otherwise returns the last tie).
    items.iter().rev().max_by_key(|item| {
        let item_language = item.get("iso_639_1").and_then(Value::as_str);
        let item_region = item.get("iso_3166_1").and_then(Value::as_str);
        (
            item_language == Some(language_code) && item_region == region,
            item_language == Some(language_code) && item_region.is_none(),
            item_language == Some(language_code),
            item_language == Some("en"),
            item_language.is_none(),
        )
    })
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::select_localized_logo;
    use serde_json::json;

    #[test]
    fn localized_logo_preserves_tmdb_order_for_equal_candidates() {
        let logos = vec![
            json!({ "file_path": "/colored.png", "iso_639_1": "en", "iso_3166_1": null }),
            json!({ "file_path": "/black.png", "iso_639_1": "en", "iso_3166_1": null }),
        ];

        let selected = select_localized_logo(&logos, "en")
            .and_then(|logo| logo.get("file_path"))
            .and_then(|path| path.as_str());

        assert_eq!(selected, Some("/colored.png"));
    }

    #[test]
    fn localized_logo_still_prefers_an_explicit_region() {
        let logos = vec![
            json!({ "file_path": "/generic.png", "iso_639_1": "en", "iso_3166_1": null }),
            json!({ "file_path": "/us.png", "iso_639_1": "en", "iso_3166_1": "US" }),
        ];

        let selected = select_localized_logo(&logos, "en-US")
            .and_then(|logo| logo.get("file_path"))
            .and_then(|path| path.as_str());

        assert_eq!(selected, Some("/us.png"));
    }
}
fn image(value: &Value, key: &str, size: &str) -> Option<String> {
    string(value, key).map(|path| format!("https://image.tmdb.org/t/p/{size}{path}"))
}
fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
fn is_series(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "series" | "tv" | "show" | "tvshow" | "anime"
    )
}

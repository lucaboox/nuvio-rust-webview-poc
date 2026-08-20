use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use anyhow::{Context, Result};
use rayon::join;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const INTRO_DB_URL: &str = "https://api.introdb.app/segments";
const ANI_SKIP_URL: &str = "https://api.aniskip.com/v2/skip-times";
const ARM_URL: &str = "https://arm.haglund.dev/api/v2";
const ANIME_SKIP_URL: &str = "https://api.anime-skip.com/graphql";
const MAX_CACHE_ENTRIES: usize = 512;

static HTTP: OnceLock<Client> = OnceLock::new();
static SEGMENT_CACHE: OnceLock<Mutex<HashMap<String, Vec<SkipSegment>>>> = OnceLock::new();
static ARM_CACHE: OnceLock<Mutex<HashMap<String, Vec<ArmEntry>>>> = OnceLock::new();
static ANIME_SHOW_CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

#[derive(Clone, Default)]
pub struct SkipOptions {
    pub anime_skip_enabled: bool,
    pub anime_skip_client_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkipSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(rename = "type")]
    pub segment_type: String,
    pub provider: String,
}

#[derive(Debug, Default, Deserialize)]
struct IntroDbResponse {
    intro: Option<IntroDbSegment>,
    recap: Option<IntroDbSegment>,
    outro: Option<IntroDbSegment>,
}

#[derive(Debug, Deserialize)]
struct IntroDbSegment {
    #[serde(default)]
    start_sec: Option<Value>,
    #[serde(default)]
    end_sec: Option<Value>,
    #[serde(default)]
    start_ms: Option<i64>,
    #[serde(default)]
    end_ms: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct AniSkipResponse {
    found: bool,
    results: Option<Vec<AniSkipResult>>,
}

#[derive(Debug, Deserialize)]
struct AniSkipResult {
    interval: AniSkipInterval,
    #[serde(rename = "skipType")]
    skip_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AniSkipInterval {
    start_time: f64,
    end_time: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct ArmEntry {
    myanimelist: Option<i64>,
    anilist: Option<i64>,
    kitsu: Option<i64>,
    imdb: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AnimeSkipResponse {
    data: Option<AnimeSkipData>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnimeSkipData {
    find_shows_by_external_id: Option<Vec<AnimeSkipShow>>,
    find_episodes_by_show_id: Option<Vec<AnimeSkipEpisode>>,
}

#[derive(Debug, Deserialize)]
struct AnimeSkipShow {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AnimeSkipEpisode {
    season: Option<String>,
    number: Option<String>,
    timestamps: Option<Vec<AnimeSkipTimestamp>>,
}

#[derive(Debug, Deserialize)]
struct AnimeSkipTimestamp {
    at: f64,
    #[serde(rename = "type")]
    timestamp_type: AnimeSkipTimestampType,
}

#[derive(Debug, Deserialize)]
struct AnimeSkipTimestampType {
    name: String,
}

/// Resolves the providers that do not require credentials. The settings-aware
/// path below also enables Anime-Skip using its separately synced client ID.
pub fn resolve(
    content_id: &str,
    video_id: &str,
    season: i64,
    episode: i64,
) -> Result<Vec<SkipSegment>> {
    resolve_with_options(
        content_id,
        video_id,
        season,
        episode,
        &SkipOptions::default(),
    )
}

pub fn resolve_with_options(
    content_id: &str,
    video_id: &str,
    season: i64,
    episode: i64,
    options: &SkipOptions,
) -> Result<Vec<SkipSegment>> {
    if season < 0 || episode < 1 {
        return Ok(Vec::new());
    }
    let identity = media_identity(content_id, video_id);
    let anime_enabled =
        options.anime_skip_enabled && !options.anime_skip_client_id.trim().is_empty();
    let credential_revision = anime_enabled
        .then(|| credential_fingerprint(&options.anime_skip_client_id))
        .unwrap_or_default();
    let cache_key = format!(
        "{}:{season}:{episode}:anime={anime_enabled}:{credential_revision:x}",
        identity.cache_key()
    );
    if let Some(cached) = cache_get(&cache_key) {
        return Ok(cached);
    }
    let client = http_client()?;
    let result = match identity {
        MediaIdentity::Imdb(id) => resolve_for_imdb(client, &id, season, episode, options),
        MediaIdentity::Mal(id) => resolve_for_mal(client, &id, season, episode, options),
        MediaIdentity::Kitsu(id) => resolve_for_kitsu(client, &id, season, episode, options),
        MediaIdentity::Unknown => Vec::new(),
    };
    cache_put(cache_key, result.clone());
    Ok(result)
}

#[derive(Debug)]
enum MediaIdentity {
    Imdb(String),
    Mal(String),
    Kitsu(String),
    Unknown,
}

impl MediaIdentity {
    fn cache_key(&self) -> String {
        match self {
            Self::Imdb(value) => format!("imdb:{value}"),
            Self::Mal(value) => format!("mal:{value}"),
            Self::Kitsu(value) => format!("kitsu:{value}"),
            Self::Unknown => "unknown".to_string(),
        }
    }
}

fn media_identity(content_id: &str, video_id: &str) -> MediaIdentity {
    for identity in [content_id, video_id] {
        let base = identity.split(':').next().unwrap_or(identity).trim();
        if base.starts_with("tt")
            && base.len() > 2
            && base[2..]
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            return MediaIdentity::Imdb(base.to_string());
        }
        if let Some(value) = prefixed_id(identity, "mal:") {
            return MediaIdentity::Mal(value);
        }
        if let Some(value) = prefixed_id(identity, "kitsu:") {
            return MediaIdentity::Kitsu(value);
        }
    }
    MediaIdentity::Unknown
}

fn prefixed_id(identity: &str, prefix: &str) -> Option<String> {
    identity
        .strip_prefix(prefix)
        .and_then(|value| value.split(':').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| value.chars().all(|character| character.is_ascii_digit()))
        .map(str::to_string)
}

fn resolve_for_imdb(
    client: &Client,
    imdb_id: &str,
    season: i64,
    episode: i64,
    options: &SkipOptions,
) -> Vec<SkipSegment> {
    // Official Nuvio starts IntroDB and ARM together, then fills missing
    // categories from Anime-Skip and AniSkip in parallel.
    let (intro_db, entries) = join(
        || resolve_intro_db(client, imdb_id, season, episode).unwrap_or_default(),
        || resolve_imdb_entries(client, imdb_id),
    );
    let season_entry = usize::try_from(season.saturating_sub(1))
        .ok()
        .and_then(|index| entries.get(index));
    let mal_id = season_entry
        .and_then(|entry| entry.myanimelist)
        .or_else(|| entries.first().and_then(|entry| entry.myanimelist));
    let (anime_skip, ani_skip) = join(
        || resolve_anime_skip_for_entries(client, &entries, season, episode, options),
        || {
            mal_id
                .map(|id| resolve_ani_skip(client, &id.to_string(), episode).unwrap_or_default())
                .unwrap_or_default()
        },
    );
    merge_by_priority([intro_db, anime_skip, ani_skip])
}

fn resolve_for_mal(
    client: &Client,
    mal_id: &str,
    season: i64,
    episode: i64,
    options: &SkipOptions,
) -> Vec<SkipSegment> {
    let (ani_skip, imdb_entry) = join(
        || resolve_ani_skip(client, mal_id, episode).unwrap_or_default(),
        || resolve_arm_entry(client, "myanimelist", mal_id, "imdb"),
    );
    let (intro_db, anime_skip) = if let Some(imdb_id) = imdb_entry.and_then(|entry| entry.imdb) {
        let entries = resolve_imdb_entries(client, &imdb_id);
        let resolved_season = entries
            .iter()
            .position(|entry| entry.myanimelist.map(|id| id.to_string()).as_deref() == Some(mal_id))
            .and_then(|index| i64::try_from(index + 1).ok())
            .unwrap_or(season.max(1));
        join(
            || resolve_intro_db(client, &imdb_id, resolved_season, episode).unwrap_or_default(),
            || resolve_anime_skip_for_entries(client, &entries, resolved_season, episode, options),
        )
    } else {
        let anilist_id = resolve_arm_entry(client, "myanimelist", mal_id, "anilist")
            .and_then(|entry| entry.anilist)
            .map(|id| id.to_string());
        (
            Vec::new(),
            anilist_id
                .map(|id| resolve_anime_skip(client, &id, episode, None, options))
                .unwrap_or_default(),
        )
    };
    merge_by_priority([intro_db, anime_skip, ani_skip])
}

fn resolve_for_kitsu(
    client: &Client,
    kitsu_id: &str,
    season: i64,
    episode: i64,
    options: &SkipOptions,
) -> Vec<SkipSegment> {
    let (mal_entry, imdb_entry) = join(
        || resolve_arm_entry(client, "kitsu", kitsu_id, "myanimelist"),
        || resolve_arm_entry(client, "kitsu", kitsu_id, "imdb"),
    );
    let mal_id = mal_entry
        .and_then(|entry| entry.myanimelist)
        .map(|id| id.to_string());
    let ani_skip = mal_id
        .as_deref()
        .map(|id| resolve_ani_skip(client, id, episode).unwrap_or_default())
        .unwrap_or_default();
    let (intro_db, anime_skip) = if let Some(imdb_id) = imdb_entry.and_then(|entry| entry.imdb) {
        let entries = resolve_imdb_entries(client, &imdb_id);
        let resolved_season = entries
            .iter()
            .position(|entry| entry.kitsu.map(|id| id.to_string()).as_deref() == Some(kitsu_id))
            .and_then(|index| i64::try_from(index + 1).ok())
            .unwrap_or(season.max(1));
        join(
            || resolve_intro_db(client, &imdb_id, resolved_season, episode).unwrap_or_default(),
            || resolve_anime_skip_for_entries(client, &entries, resolved_season, episode, options),
        )
    } else {
        let anilist_id = resolve_arm_entry(client, "kitsu", kitsu_id, "anilist")
            .and_then(|entry| entry.anilist)
            .map(|id| id.to_string());
        (
            Vec::new(),
            anilist_id
                .map(|id| resolve_anime_skip(client, &id, episode, None, options))
                .unwrap_or_default(),
        )
    };
    merge_by_priority([intro_db, anime_skip, ani_skip])
}

fn http_client() -> Result<&'static Client> {
    if let Some(client) = HTTP.get() {
        return Ok(client);
    }
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .user_agent("NuvioDesktop/0.1")
        .build()
        .context("could not create the skip-segment client")?;
    let _ = HTTP.set(client);
    HTTP.get()
        .context("skip-segment client was not initialized")
}

fn resolve_intro_db(
    client: &Client,
    imdb_id: &str,
    season: i64,
    episode: i64,
) -> Result<Vec<SkipSegment>> {
    let response = client
        .get(INTRO_DB_URL)
        .query(&[
            ("imdb_id", imdb_id.to_string()),
            ("season", season.to_string()),
            ("episode", episode.to_string()),
        ])
        .send()
        .context("IntroDB could not be reached")?
        .error_for_status()
        .context("IntroDB rejected the segment request")?
        .json::<IntroDbResponse>()
        .context("IntroDB returned an unsupported response")?;
    Ok([
        ("intro", response.intro),
        ("recap", response.recap),
        ("outro", response.outro),
    ]
    .into_iter()
    .filter_map(|(segment_type, segment)| {
        segment.and_then(|segment| intro_segment(segment_type, segment))
    })
    .collect())
}

fn resolve_ani_skip(client: &Client, mal_id: &str, episode: i64) -> Result<Vec<SkipSegment>> {
    let response = client
        .get(format!("{ANI_SKIP_URL}/{mal_id}/{episode}"))
        .query(&[
            ("types", "op"),
            ("types", "ed"),
            ("types", "recap"),
            ("types", "mixed-op"),
            ("types", "mixed-ed"),
            ("episodeLength", "0"),
        ])
        .send()
        .context("AniSkip could not be reached")?
        .error_for_status()
        .context("AniSkip rejected the segment request")?
        .json::<AniSkipResponse>()
        .context("AniSkip returned an unsupported response")?;
    if !response.found {
        return Ok(Vec::new());
    }
    Ok(response
        .results
        .unwrap_or_default()
        .into_iter()
        .filter_map(|result| {
            from_seconds(
                result.interval.start_time,
                result.interval.end_time,
                normalize_type(&result.skip_type),
                "aniskip",
            )
        })
        .collect())
}

fn resolve_imdb_entries(client: &Client, imdb_id: &str) -> Vec<ArmEntry> {
    if let Some(cached) = ARM_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|cache| cache.get(imdb_id).cloned())
    {
        return cached;
    }
    let entries = client
        .get(format!("{ARM_URL}/imdb"))
        .query(&[("id", imdb_id), ("include", "myanimelist,anilist,kitsu")])
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::json::<Vec<ArmEntry>>)
        .unwrap_or_default();
    cache_insert(
        ARM_CACHE.get_or_init(|| Mutex::new(HashMap::new())),
        imdb_id.to_string(),
        entries.clone(),
    );
    entries
}

fn resolve_arm_entry(
    client: &Client,
    source: &str,
    source_id: &str,
    include: &str,
) -> Option<ArmEntry> {
    client
        .get(format!("{ARM_URL}/ids"))
        .query(&[("source", source), ("id", source_id), ("include", include)])
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::json::<ArmEntry>)
        .ok()
}

fn resolve_anime_skip_for_entries(
    client: &Client,
    entries: &[ArmEntry],
    season: i64,
    episode: i64,
    options: &SkipOptions,
) -> Vec<SkipSegment> {
    let season_id = usize::try_from(season.saturating_sub(1))
        .ok()
        .and_then(|index| entries.get(index))
        .and_then(|entry| entry.anilist)
        .map(|id| id.to_string());
    let fallback_id = entries
        .first()
        .and_then(|entry| entry.anilist)
        .map(|id| id.to_string());
    if let Some(id) = season_id.as_deref() {
        let result = resolve_anime_skip(client, id, episode, None, options);
        if !result.is_empty() {
            return result;
        }
    }
    if let Some(id) = fallback_id.as_deref()
        && Some(id) != season_id.as_deref()
    {
        return resolve_anime_skip(client, id, episode, Some(season), options);
    }
    Vec::new()
}

fn resolve_anime_skip(
    client: &Client,
    anilist_id: &str,
    episode: i64,
    season: Option<i64>,
    options: &SkipOptions,
) -> Vec<SkipSegment> {
    let client_id = options.anime_skip_client_id.trim();
    if !options.anime_skip_enabled || client_id.is_empty() {
        return Vec::new();
    }
    let Some(show_id) = resolve_anime_show_id(client, anilist_id, client_id) else {
        return Vec::new();
    };
    let query = format!(
        "{{ findEpisodesByShowId(showId: \"{show_id}\") {{ season number timestamps {{ at type {{ name }} }} }} }}"
    );
    let response = anime_skip_query(client, client_id, &query).unwrap_or_default();
    let target = response
        .data
        .and_then(|data| data.find_episodes_by_show_id)
        .unwrap_or_default()
        .into_iter()
        .find(|candidate| {
            candidate.number.as_deref().and_then(parse_integer) == Some(episode)
                && season.is_none_or(|value| {
                    candidate.season.as_deref().and_then(parse_integer) == Some(value)
                })
        });
    let mut timestamps = target
        .and_then(|value| value.timestamps)
        .unwrap_or_default();
    timestamps.sort_by(|left, right| left.at.total_cmp(&right.at));
    timestamps
        .iter()
        .enumerate()
        .filter_map(|(index, timestamp)| {
            let segment_type = match timestamp
                .timestamp_type
                .name
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "intro" | "new intro" => "intro",
                "credits" => "outro",
                "recap" => "recap",
                _ => return None,
            };
            let end = timestamps
                .get(index + 1)
                .map(|next| next.at)
                .unwrap_or(f64::MAX);
            from_seconds(timestamp.at, end, segment_type, "animeskip")
        })
        .collect()
}

fn resolve_anime_show_id(client: &Client, anilist_id: &str, client_id: &str) -> Option<String> {
    // A rejected client ID can legitimately produce an empty response. Scope
    // that negative cache entry to a non-reversible fingerprint so saving a
    // corrected credential retries immediately without retaining the secret in
    // a process-wide map key.
    let cache_key = format!("{anilist_id}:{:x}", credential_fingerprint(client_id));
    if let Some(cached) = ANIME_SHOW_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())
    {
        return cached;
    }
    let query = format!(
        "{{ findShowsByExternalId(service: ANILIST, serviceId: \"{anilist_id}\") {{ id }} }}"
    );
    let shows = anime_skip_query(client, client_id, &query)
        .ok()
        .and_then(|response| response.data)
        .and_then(|data| data.find_shows_by_external_id)
        .unwrap_or_default();
    let resolved = (shows.len() == 1).then(|| shows[0].id.clone());
    if shows.len() <= 1 {
        cache_insert(
            ANIME_SHOW_CACHE.get_or_init(|| Mutex::new(HashMap::new())),
            cache_key,
            resolved.clone(),
        );
    }
    resolved
}

fn credential_fingerprint(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.trim().hash(&mut hasher);
    hasher.finish()
}

fn anime_skip_query(client: &Client, client_id: &str, query: &str) -> Result<AnimeSkipResponse> {
    client
        .post(ANIME_SKIP_URL)
        .header("X-Client-ID", client_id)
        .json(&json!({ "query": query }))
        .send()
        .context("Anime-Skip could not be reached")?
        .error_for_status()
        .context("Anime-Skip rejected the segment request")?
        .json::<AnimeSkipResponse>()
        .context("Anime-Skip returned an unsupported response")
}

fn merge_by_priority<const N: usize>(providers: [Vec<SkipSegment>; N]) -> Vec<SkipSegment> {
    let mut selected: HashMap<&'static str, SkipSegment> = HashMap::new();
    for segments in providers {
        for segment in segments {
            let Some(category) = segment_category(&segment.segment_type) else {
                continue;
            };
            selected.entry(category).or_insert_with(|| SkipSegment {
                segment_type: category.to_string(),
                ..segment
            });
        }
    }
    ["intro", "recap", "outro"]
        .into_iter()
        .filter_map(|category| selected.remove(category))
        .collect()
}

fn segment_category(segment_type: &str) -> Option<&'static str> {
    match segment_type.trim().to_ascii_lowercase().as_str() {
        "intro" | "op" | "mixed-op" => Some("intro"),
        "outro" | "ed" | "mixed-ed" | "credits" | "ending" => Some("outro"),
        "recap" => Some("recap"),
        _ => None,
    }
}

fn cache_get(key: &str) -> Option<Vec<SkipSegment>> {
    SEGMENT_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|cache| cache.get(key).cloned())
}

fn cache_put(key: String, value: Vec<SkipSegment>) {
    cache_insert(
        SEGMENT_CACHE.get_or_init(|| Mutex::new(HashMap::new())),
        key,
        value,
    );
}

fn cache_insert<T: Clone>(cache: &Mutex<HashMap<String, T>>, key: String, value: T) {
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(key, value);
    }
}

fn intro_segment(segment_type: &str, segment: IntroDbSegment) -> Option<SkipSegment> {
    let start_ms = segment.start_ms.or_else(|| {
        segment
            .start_sec
            .as_ref()
            .and_then(parse_seconds)
            .map(to_ms)
    })?;
    let end_ms = segment
        .end_ms
        .or_else(|| segment.end_sec.as_ref().and_then(parse_seconds).map(to_ms))?;
    valid_segment(start_ms, end_ms, segment_type, "introdb")
}

fn from_seconds(start: f64, end: f64, segment_type: &str, provider: &str) -> Option<SkipSegment> {
    valid_segment(to_ms(start), to_ms(end), segment_type, provider)
}

fn valid_segment(
    start_ms: i64,
    end_ms: i64,
    segment_type: &str,
    provider: &str,
) -> Option<SkipSegment> {
    (start_ms >= 0 && end_ms > start_ms).then(|| SkipSegment {
        start_ms,
        end_ms,
        segment_type: segment_type.to_string(),
        provider: provider.to_string(),
    })
}

fn parse_seconds(value: &Value) -> Option<f64> {
    if let Some(seconds) = value.as_f64() {
        return Some(seconds);
    }
    let text = value.as_str()?.trim();
    if let Ok(seconds) = text.parse::<f64>() {
        return Some(seconds);
    }
    let fields = text
        .split(':')
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    match fields.as_slice() {
        [minutes, seconds] => Some(minutes * 60.0 + seconds),
        [hours, minutes, seconds] => Some(hours * 3600.0 + minutes * 60.0 + seconds),
        _ => None,
    }
}

fn parse_integer(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok()
}

fn to_ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

fn normalize_type(segment_type: &str) -> &str {
    segment_category(segment_type).unwrap_or("skip")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numeric_and_clock_times() {
        assert_eq!(parse_seconds(&Value::from(12.5)), Some(12.5));
        assert_eq!(parse_seconds(&Value::from("2:03")), Some(123.0));
        assert_eq!(parse_seconds(&Value::from("1:02:03")), Some(3723.0));
    }

    #[test]
    fn rejects_empty_or_backwards_segments() {
        assert!(valid_segment(2000, 1000, "intro", "test").is_none());
        assert!(valid_segment(1000, 1000, "intro", "test").is_none());
    }

    #[test]
    fn recognizes_stremio_identity_shapes() {
        assert!(matches!(
            media_identity("tt0944947", "tt0944947:1:1"),
            MediaIdentity::Imdb(value) if value == "tt0944947"
        ));
        assert!(matches!(
            media_identity("mal:5114", "mal:5114:1"),
            MediaIdentity::Mal(value) if value == "5114"
        ));
    }

    #[test]
    fn merges_each_category_from_the_highest_priority_provider() {
        let merged = merge_by_priority([
            vec![valid_segment(1, 2, "intro", "introdb").unwrap()],
            vec![
                valid_segment(3, 4, "op", "animeskip").unwrap(),
                valid_segment(8, 9, "ed", "animeskip").unwrap(),
            ],
            vec![valid_segment(10, 11, "recap", "aniskip").unwrap()],
        ]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].provider, "introdb");
        assert_eq!(merged[1].provider, "aniskip");
        assert_eq!(merged[2].provider, "animeskip");
    }
}

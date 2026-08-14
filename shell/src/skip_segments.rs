use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const INTRO_DB_URL: &str = "https://api.introdb.app/segments";
const ANI_SKIP_URL: &str = "https://api.aniskip.com/v2/skip-times";

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

pub fn resolve(
    content_id: &str,
    video_id: &str,
    season: i64,
    episode: i64,
) -> Result<Vec<SkipSegment>> {
    if season < 0 || episode < 1 {
        return Ok(Vec::new());
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("NuvioRustPoc/0.2.0")
        .build()
        .context("could not create the skip-segment client")?;
    let imdb_id = [content_id, video_id]
        .into_iter()
        .map(|identity| identity.split(':').next().unwrap_or(identity))
        .find(|identity| identity.starts_with("tt"));
    if let Some(imdb_id) = imdb_id {
        return resolve_intro_db(&client, imdb_id, season, episode);
    }
    let mal_id = content_id
        .strip_prefix("mal:")
        .or_else(|| video_id.strip_prefix("mal:"))
        .and_then(|value| value.split(':').next());
    if let Some(mal_id) = mal_id {
        return resolve_ani_skip(&client, mal_id, episode);
    }
    Ok(Vec::new())
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

fn to_ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

fn normalize_type(segment_type: &str) -> &str {
    match segment_type.to_ascii_lowercase().as_str() {
        "op" | "mixed-op" => "intro",
        "ed" | "mixed-ed" | "credits" | "ending" => "outro",
        "recap" => "recap",
        _ => "skip",
    }
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
}

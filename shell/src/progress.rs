use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::AuthService;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePoint {
    pub content_id: String,
    pub content_type: String,
    pub video_id: String,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub last_watched: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedItem {
    pub content_id: String,
    pub content_type: String,
    pub title: String,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub watched_at: i64,
}

pub fn list(auth: &AuthService, profile_id: i32) -> Result<Vec<ResumePoint>> {
    let value = auth.rpc_value(
        "sync_pull_watch_progress",
        &json!({ "p_profile_id": profile_id, "p_limit": 1000 }),
    )?;
    Ok(value
        .as_array()
        .context("watch progress response was not a list")?
        .iter()
        .filter_map(parse)
        .collect())
}

pub fn watched(auth: &AuthService, profile_id: i32) -> Result<Vec<WatchedItem>> {
    let mut all = Vec::new();
    let mut page = 1;
    loop {
        let value = auth.rpc_value(
            "sync_pull_watched_items",
            &json!({ "p_profile_id": profile_id, "p_page": page, "p_page_size": 200 }),
        )?;
        let rows = value
            .as_array()
            .context("watched items response was not a list")?;
        for row in rows {
            let Some(content_id) = row.get("content_id").and_then(Value::as_str) else {
                continue;
            };
            all.push(WatchedItem {
                content_id: content_id.to_string(),
                content_type: row
                    .get("content_type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                title: row
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                season: number(row, "season"),
                episode: number(row, "episode"),
                watched_at: number(row, "watched_at").unwrap_or_default(),
            });
        }
        if rows.len() < 200 {
            break;
        }
        page += 1;
    }
    Ok(all)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackIdentity {
    pub content_id: String,
    pub content_type: String,
    pub video_id: String,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}

/// Nuvio's `WatchingPolicies`: a row is only worth storing past a second of
/// playback, and counts as finished at 90% or when the player reports EOF.
const PROGRESS_STORE_THRESHOLD_MS: i64 = 1_000;
const COMPLETION_THRESHOLD_FRACTION: f64 = 0.90;

/// Mirrors `buildWatchProgressKey`.
fn build_progress_key(content_id: &str, season: Option<i64>, episode: Option<i64>) -> String {
    match (season, episode) {
        (Some(season), Some(episode)) => format!("{content_id}_s{season}e{episode}"),
        _ => content_id.to_string(),
    }
}

/// Mirrors `isProgressComplete`.
fn is_complete(position_ms: i64, duration_ms: i64, is_ended: bool) -> bool {
    if is_ended {
        return true;
    }
    if duration_ms <= 0 {
        return false;
    }
    position_ms as f64 / duration_ms as f64 >= COMPLETION_THRESHOLD_FRACTION
}

/// A progress row as the server holds it, including the opaque key.
#[derive(Clone, Debug)]
struct StoredProgress {
    progress_key: String,
    content_id: String,
    video_id: String,
    season: Option<i64>,
    episode: Option<i64>,
    position_ms: i64,
    duration_ms: i64,
    last_watched: i64,
}

/// Mirrors `watchProgressEntryFreshnessComparator`.
fn freshness(row: &StoredProgress) -> (i64, i64, i64, &str) {
    (
        row.last_watched,
        row.position_ms,
        row.duration_ms,
        row.video_id.as_str(),
    )
}

/// Mirrors `resolveIdentityForUpsert`. The server's progress key is opaque —
/// two clients can disagree about what the computed key *should* be for the
/// same episode, so an existing row's key always wins. Recomputing it would
/// insert a duplicate row rather than updating the one already there.
fn resolve_progress_key(rows: &[StoredProgress], identity: &PlaybackIdentity) -> String {
    let logical: Vec<&StoredProgress> = rows
        .iter()
        .filter(|row| {
            row.content_id == identity.content_id
                && row.season == identity.season
                && row.episode == identity.episode
        })
        .collect();

    // An exact playback-id match wins; freshness breaks the remaining ties.
    logical
        .iter()
        .copied()
        .filter(|row| row.video_id == identity.video_id)
        .max_by_key(|row| freshness(row))
        .or_else(|| logical.iter().copied().max_by_key(|row| freshness(row)))
        .map(|row| row.progress_key.trim().to_string())
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| {
            build_progress_key(&identity.content_id, identity.season, identity.episode)
        })
}

fn stored_rows(auth: &AuthService, profile_id: i32) -> Result<Vec<StoredProgress>> {
    let value = auth.rpc_value(
        "sync_pull_watch_progress",
        &json!({ "p_profile_id": profile_id, "p_limit": 1000 }),
    )?;
    Ok(value
        .as_array()
        .context("watch progress response was not a list")?
        .iter()
        .filter_map(|row| {
            Some(StoredProgress {
                progress_key: row
                    .get("progress_key")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                content_id: row.get("content_id")?.as_str()?.to_string(),
                video_id: row
                    .get("video_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                season: number(row, "season"),
                episode: number(row, "episode"),
                position_ms: number(row, "position").unwrap_or_default(),
                duration_ms: number(row, "duration").unwrap_or_default(),
                last_watched: number(row, "last_watched").unwrap_or_default(),
            })
        })
        .collect())
}

pub fn resume(
    auth: &AuthService,
    profile_id: i32,
    content_id: &str,
) -> Result<Option<ResumePoint>> {
    Ok(list(auth, profile_id)?
        .into_iter()
        .filter(|entry| entry.content_id == content_id)
        .max_by_key(|entry| entry.last_watched))
}

/// Writes one watch-progress row, following the same store/completion rules as
/// Nuvio's `WatchProgressRepository.upsert` so both clients agree on what a
/// finished episode looks like.
///
/// `is_ended` should be true when the player reached EOF rather than the user
/// closing it early; Nuvio treats that as complete regardless of position.
pub fn push(
    auth: &AuthService,
    profile_id: i32,
    identity: &PlaybackIdentity,
    position_ms: i64,
    duration_ms: i64,
    is_ended: bool,
) -> Result<()> {
    let position_ms = position_ms.max(0);
    let duration_ms = duration_ms.max(0);
    let completed = is_complete(position_ms, duration_ms, is_ended);
    if !completed && position_ms < PROGRESS_STORE_THRESHOLD_MS {
        return Ok(());
    }

    // Nuvio pins a finished row to the full duration. Without this the phone
    // keeps the title in Continue Watching at 9x% forever and never advances to
    // the next episode.
    let stored_position = if completed && duration_ms > 0 {
        duration_ms
    } else {
        position_ms
    };

    let progress_key = resolve_progress_key(&stored_rows(auth, profile_id)?, identity);
    let last_watched = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    auth.rpc_unit(
        "sync_push_watch_progress",
        &json!({
            "p_profile_id": profile_id,
            "p_entries": [{
                "content_id": identity.content_id,
                "content_type": identity.content_type,
                "video_id": identity.video_id,
                "season": identity.season,
                "episode": identity.episode,
                "position": stored_position,
                "duration": duration_ms,
                "last_watched": last_watched,
                "progress_key": progress_key,
            }],
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )?;
    Ok(())
}

/// Marks or clears a title/episode as watched, and drops any resume point for
/// it so the two never disagree.
pub fn set_watched(
    auth: &AuthService,
    profile_id: i32,
    identity: &PlaybackIdentity,
    title: &str,
    watched: bool,
) -> Result<()> {
    let key = json!({
        "content_id": identity.content_id,
        "season": identity.season,
        "episode": identity.episode,
    });
    if watched {
        auth.rpc_unit(
            "sync_push_watched_items",
            &json!({
                "p_profile_id": profile_id,
                "p_items": [{
                    "content_id": identity.content_id,
                    "content_type": identity.content_type,
                    "title": title,
                    "season": identity.season,
                    "episode": identity.episode,
                    "watched_at": now_ms(),
                }],
                "p_origin_client_id": auth.sync_client_id(),
            }),
        )?;
    } else {
        auth.rpc_unit(
            "sync_delete_watched_items",
            &json!({
                "p_profile_id": profile_id,
                "p_keys": [key],
                "p_origin_client_id": auth.sync_client_id(),
            }),
        )?;
    }
    // A stale resume point would still render a progress bar under a row the
    // user just reset, so clear it either way.
    clear_progress(auth, profile_id, identity)
}

/// Removes the resume point for one video, using the same opaque key the
/// server stores.
pub fn clear_progress(
    auth: &AuthService,
    profile_id: i32,
    identity: &PlaybackIdentity,
) -> Result<()> {
    let key = resolve_progress_key(&stored_rows(auth, profile_id)?, identity);
    auth.rpc_unit(
        "sync_delete_watch_progress",
        &json!({
            "p_profile_id": profile_id,
            "p_keys": [key],
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn parse(row: &Value) -> Option<ResumePoint> {
    Some(ResumePoint {
        content_id: row.get("content_id")?.as_str()?.to_string(),
        content_type: row.get("content_type")?.as_str()?.to_string(),
        video_id: row.get("video_id")?.as_str()?.to_string(),
        season: number(row, "season"),
        episode: number(row, "episode"),
        position_ms: number(row, "position").unwrap_or_default(),
        duration_ms: number(row, "duration").unwrap_or_default(),
        last_watched: number(row, "last_watched").unwrap_or_default(),
    })
}

fn number(value: &Value, key: &str) -> Option<i64> {
    value
        .get(key)
        .and_then(|item| item.as_i64().or_else(|| item.as_str()?.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(season: Option<i64>, episode: Option<i64>, video: &str) -> PlaybackIdentity {
        PlaybackIdentity {
            content_id: "tt100".to_string(),
            content_type: "series".to_string(),
            video_id: video.to_string(),
            season,
            episode,
        }
    }

    fn stored(key: &str, video: &str, last_watched: i64) -> StoredProgress {
        StoredProgress {
            progress_key: key.to_string(),
            content_id: "tt100".to_string(),
            video_id: video.to_string(),
            season: Some(1),
            episode: Some(2),
            position_ms: 500,
            duration_ms: 1000,
            last_watched,
        }
    }

    #[test]
    fn progress_keys_match_nuvios_format() {
        assert_eq!(build_progress_key("tt100", Some(1), Some(2)), "tt100_s1e2");
        assert_eq!(build_progress_key("tt100", None, None), "tt100");
        // A half-identified episode falls back to the bare content id, exactly
        // as buildWatchProgressKey does.
        assert_eq!(build_progress_key("tt100", Some(1), None), "tt100");
    }

    #[test]
    fn an_existing_server_key_always_wins() {
        // Regression: recomputing the key would insert a second row for an
        // episode whose stored key came from another client.
        let rows = vec![stored("opaque-server-key", "v1", 10)];
        assert_eq!(
            resolve_progress_key(&rows, &identity(Some(1), Some(2), "v1")),
            "opaque-server-key"
        );
    }

    #[test]
    fn an_exact_video_match_beats_a_fresher_sibling() {
        let rows = vec![
            stored("key-other-alias", "v2", 99),
            stored("key-exact", "v1", 1),
        ];
        assert_eq!(
            resolve_progress_key(&rows, &identity(Some(1), Some(2), "v1")),
            "key-exact"
        );
    }

    #[test]
    fn without_an_exact_match_the_freshest_logical_row_wins() {
        let rows = vec![stored("older", "v8", 5), stored("newer", "v9", 50)];
        assert_eq!(
            resolve_progress_key(&rows, &identity(Some(1), Some(2), "v1")),
            "newer"
        );
    }

    #[test]
    fn unrelated_episodes_do_not_steal_a_key() {
        let rows = vec![stored("s1e2-key", "v1", 10)];
        assert_eq!(
            resolve_progress_key(&rows, &identity(Some(1), Some(3), "v3")),
            "tt100_s1e3"
        );
    }

    #[test]
    fn a_blank_stored_key_falls_back_to_the_computed_one() {
        let rows = vec![stored("   ", "v1", 10)];
        assert_eq!(
            resolve_progress_key(&rows, &identity(Some(1), Some(2), "v1")),
            "tt100_s1e2"
        );
    }

    #[test]
    fn completion_follows_nuvios_ninety_percent_rule() {
        assert!(is_complete(900, 1000, false));
        assert!(!is_complete(899, 1000, false));
        assert!(is_complete(10, 1000, true));
        assert!(!is_complete(10, 0, false));
    }
}

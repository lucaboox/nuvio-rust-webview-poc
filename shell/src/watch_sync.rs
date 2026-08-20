//! Snapshot-once, then deltas, for watch progress and watched items.
//!
//! Pulling both tables in full on every load does not scale with a long
//! history. The backend keeps an append-only event log per table, so a client
//! takes one snapshot, records the log cursor, and afterwards asks only for
//! what changed.
//!
//! Mirrors Nuvio's own `WatchedRepository`: read the cursor **before** taking
//! the snapshot. A write landing mid-snapshot is then replayed as a delta
//! rather than lost in the gap between the two calls.

use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    auth::AuthService,
    progress::{ResumePoint, WatchedItem},
};

const DELTA_PAGE_SIZE: i64 = 900;

#[derive(Default, Deserialize, Serialize)]
struct Cache {
    profile_id: i32,
    progress_cursor: i64,
    watched_cursor: i64,
    #[serde(default)]
    progress: Vec<ResumePoint>,
    #[serde(default)]
    watched: Vec<WatchedItem>,
}

fn cache_path() -> Option<PathBuf> {
    Some(dirs_next()?.join("nuvio-rust-poc").join("watch-sync.json"))
}

/// `%LOCALAPPDATA%` on Windows, `~/.local/share` elsewhere.
fn dirs_next() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
    }
}

fn read_cache(profile_id: i32) -> Option<Cache> {
    let cache: Cache = serde_json::from_slice(&std::fs::read(cache_path()?).ok()?).ok()?;
    (cache.profile_id == profile_id).then_some(cache)
}

fn write_cache(cache: &Cache) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(cache) {
        let _ = std::fs::write(path, bytes);
    }
}

fn cursor(auth: &AuthService, rpc: &str, profile_id: i32) -> Option<i64> {
    auth.rpc_value(rpc, &json!({ "p_profile_id": profile_id }))
        .ok()
        .and_then(|value| value.as_i64())
}

/// Walks the event log from `since`, handing each page to `apply`.
/// A short page means the log is caught up.
fn drain(
    auth: &AuthService,
    rpc: &str,
    profile_id: i32,
    since: i64,
    mut apply: impl FnMut(&[Value]),
) -> Result<i64> {
    let mut at = since;
    loop {
        let value = auth.rpc_value(
            rpc,
            &json!({
                "p_profile_id": profile_id,
                "p_since_event_id": at,
                "p_limit": DELTA_PAGE_SIZE,
            }),
        )?;
        let events = value.as_array().cloned().unwrap_or_default();
        if events.is_empty() {
            break;
        }
        apply(&events);
        at = events
            .iter()
            .filter_map(|event| event.get("event_id").and_then(Value::as_i64))
            .fold(at, i64::max);
        if (events.len() as i64) < DELTA_PAGE_SIZE {
            break;
        }
    }
    Ok(at)
}

fn is_delete(event: &Value) -> bool {
    event
        .get("operation")
        .and_then(Value::as_str)
        .map(|operation| operation.eq_ignore_ascii_case("delete"))
        .unwrap_or(false)
}

/// Both tables, refreshed. Falls back to a full snapshot on any surprise:
/// correct-but-slow beats a cache that has drifted from the server.
pub fn load(auth: &AuthService, profile_id: i32) -> Result<(Vec<ResumePoint>, Vec<WatchedItem>)> {
    if let Some(mut cache) = read_cache(profile_id) {
        let refreshed = (|| -> Result<()> {
            let mut progress: HashMap<String, ResumePoint> = cache
                .progress
                .drain(..)
                .map(|row| (row.progress_key.clone(), row))
                .collect();
            cache.progress_cursor = drain(
                auth,
                "sync_pull_watch_progress_delta",
                profile_id,
                cache.progress_cursor,
                |events| {
                    for event in events {
                        let key = event
                            .get("progress_key")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if is_delete(event) {
                            progress.remove(&key);
                        } else if let Some(row) = crate::progress::parse_row(event) {
                            progress.insert(key, row);
                        }
                    }
                },
            )?;

            let mut watched: HashMap<String, WatchedItem> = cache
                .watched
                .drain(..)
                .map(|item| (watched_key(&item), item))
                .collect();
            cache.watched_cursor = drain(
                auth,
                "sync_pull_watched_items_delta",
                profile_id,
                cache.watched_cursor,
                |events| {
                    for event in events {
                        let Some(item) = crate::progress::parse_watched(event) else {
                            continue;
                        };
                        if is_delete(event) {
                            watched.remove(&watched_key(&item));
                        } else {
                            watched.insert(watched_key(&item), item);
                        }
                    }
                },
            )?;

            cache.progress = progress.into_values().collect();
            cache.watched = watched.into_values().collect();
            Ok(())
        })();
        if refreshed.is_ok() {
            write_cache(&cache);
            return Ok((cache.progress, cache.watched));
        }
    }

    // Cursor first, so a write during the snapshot is replayed rather than lost.
    let progress_cursor = cursor(auth, "sync_get_watch_progress_delta_cursor", profile_id);
    let watched_cursor = cursor(auth, "sync_get_watched_items_delta_cursor", profile_id);
    let progress = crate::progress::list(auth, profile_id)?;
    let watched = crate::progress::watched(auth, profile_id)?;
    if let (Some(progress_cursor), Some(watched_cursor)) = (progress_cursor, watched_cursor) {
        write_cache(&Cache {
            profile_id,
            progress_cursor,
            watched_cursor,
            progress: progress.clone(),
            watched: watched.clone(),
        });
    }
    Ok((progress, watched))
}

fn watched_key(item: &WatchedItem) -> String {
    format!(
        "{}:{}:{}",
        item.content_id,
        item.season.unwrap_or(-1),
        item.episode.unwrap_or(-1)
    )
}

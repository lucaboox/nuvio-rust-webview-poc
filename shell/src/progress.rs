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

pub fn push(
    auth: &AuthService,
    profile_id: i32,
    identity: &PlaybackIdentity,
    position_ms: i64,
    duration_ms: i64,
) -> Result<()> {
    if position_ms <= 0 || duration_ms <= 0 {
        return Ok(());
    }
    let progress_key = match (identity.season, identity.episode) {
        (Some(season), Some(episode)) => format!("{}_s{}e{}", identity.content_id, season, episode),
        _ => identity.content_id.clone(),
    };
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
                "position": position_ms,
                "duration": duration_ms,
                "last_watched": last_watched,
                "progress_key": progress_key,
            }],
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )?;
    Ok(())
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

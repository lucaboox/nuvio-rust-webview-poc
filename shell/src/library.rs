use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{auth::AuthService, content::ContentMeta};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: String,
    pub content_type: String,
    pub name: String,
    pub poster: Option<String>,
    pub poster_shape: Option<String>,
    pub background: Option<String>,
    pub description: Option<String>,
    pub release_info: Option<String>,
    pub imdb_rating: Option<String>,
    pub genres: Vec<String>,
    pub source_manifest_url: String,
    pub addon_name: String,
    pub added_at: i64,
    pub banner: Option<String>,
    pub logo: Option<String>,
    pub released: Option<String>,
    pub runtime: Option<String>,
    pub cast: Vec<Value>,
    pub director: Vec<String>,
    pub writer: Vec<String>,
    pub trailers: Vec<Value>,
    pub videos: Vec<Value>,
    pub has_scheduled_videos: bool,
}

pub fn list(auth: &AuthService, profile_id: i32) -> Result<Vec<LibraryItem>> {
    let mut all = Vec::new();
    let mut offset = 0;
    loop {
        let value = auth.rpc_value(
            "sync_pull_library",
            &json!({ "p_profile_id": profile_id, "p_limit": 200, "p_offset": offset }),
        )?;
        let rows = value
            .as_array()
            .context("library response was not a list")?;
        for row in rows {
            all.push(parse_row(row));
        }
        if rows.len() < 200 {
            break;
        }
        offset += rows.len();
    }
    Ok(all)
}

pub fn add(auth: &AuthService, profile_id: i32, item: &ContentMeta) -> Result<()> {
    let added_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    auth.rpc_unit(
        "sync_push_library_items",
        &json!({
            "p_profile_id": profile_id,
            "p_items": [{
                "content_id": item.id,
                "content_type": item.content_type,
                "name": item.name,
                "poster": item.poster,
                "poster_shape": item.poster_shape.as_deref().unwrap_or("POSTER").to_uppercase(),
                "background": item.background.as_ref().or(item.banner.as_ref()),
                "description": item.description,
                "release_info": item.release_info,
                "imdb_rating": item.imdb_rating.as_deref().and_then(|value| value.parse::<f64>().ok()),
                "genres": item.genres,
                "addon_base_url": item.source_manifest_url,
                "added_at": added_at,
            }],
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )?;
    Ok(())
}

pub fn remove(auth: &AuthService, profile_id: i32, content_type: &str, id: &str) -> Result<()> {
    auth.rpc_unit(
        "sync_delete_library_items",
        &json!({
            "p_profile_id": profile_id,
            "p_keys": [{ "content_id": id, "content_type": content_type }],
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )?;
    Ok(())
}

fn parse_row(row: &Value) -> LibraryItem {
    let text = |key: &str| {
        row.get(key).and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
    };
    let manifest = text("addon_base_url").unwrap_or_default();
    let source_manifest_url = if manifest.is_empty() || manifest.contains("manifest.json") {
        manifest
    } else {
        format!("{}/manifest.json", manifest.trim_end_matches('/'))
    };
    LibraryItem {
        id: text("content_id").unwrap_or_default(),
        content_type: text("content_type").unwrap_or_default(),
        name: text("name").unwrap_or_default(),
        poster: text("poster"),
        poster_shape: text("poster_shape"),
        background: text("background"),
        description: text("description"),
        release_info: text("release_info"),
        imdb_rating: text("imdb_rating"),
        genres: row
            .get("genres")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        source_manifest_url,
        addon_name: String::new(),
        added_at: row
            .get("added_at")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        banner: None,
        logo: None,
        released: None,
        runtime: None,
        cast: Vec::new(),
        director: Vec::new(),
        writer: Vec::new(),
        trailers: Vec::new(),
        videos: Vec::new(),
        has_scheduled_videos: false,
    }
}

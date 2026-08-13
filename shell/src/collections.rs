use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::AuthService;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSource {
    #[serde(default = "default_provider")]
    pub provider: String,
    pub addon_id: Option<String>,
    #[serde(rename = "type")]
    pub content_type: Option<String>,
    pub catalog_id: Option<String>,
    pub genre: Option<String>,
    pub tmdb_source_type: Option<String>,
    pub title: Option<String>,
    pub tmdb_id: Option<i64>,
    pub trakt_list_id: Option<i64>,
    pub media_type: Option<String>,
    pub sort_by: Option<String>,
    pub sort_how: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionCatalogSource {
    pub addon_id: String,
    #[serde(rename = "type")]
    pub content_type: String,
    pub catalog_id: String,
    pub genre: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionFolder {
    pub id: String,
    pub title: String,
    pub cover_image_url: Option<String>,
    pub focus_gif_url: Option<String>,
    #[serde(default = "default_true")]
    pub focus_gif_enabled: bool,
    pub cover_emoji: Option<String>,
    #[serde(default = "default_tile_shape")]
    pub tile_shape: String,
    #[serde(default)]
    pub hide_title: bool,
    #[serde(default)]
    pub sources: Vec<CollectionSource>,
    #[serde(default)]
    pub catalog_sources: Vec<CollectionCatalogSource>,
    pub hero_backdrop_url: Option<String>,
    pub hero_video_url: Option<String>,
    pub title_logo_url: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub title: String,
    pub backdrop_image_url: Option<String>,
    #[serde(default)]
    pub pin_to_top: bool,
    #[serde(default = "default_view_mode")]
    pub view_mode: String,
    #[serde(default = "default_true")]
    pub show_all_tab: bool,
    #[serde(default)]
    pub folders: Vec<CollectionFolder>,
}

pub fn list(auth: &AuthService, profile_id: i32) -> Result<Vec<Collection>> {
    let payload = raw_payload(auth, profile_id)?;
    serde_json::from_value(payload).context("collections payload did not match Nuvio's schema")
}

pub fn reorder(
    auth: &AuthService,
    profile_id: i32,
    collection_id: &str,
    folder_id: Option<&str>,
    direction: i64,
) -> Result<Vec<Collection>> {
    anyhow::ensure!(
        matches!(direction, -1 | 1),
        "collection direction must be -1 or 1"
    );
    let mut payload = raw_payload(auth, profile_id)?;
    let collections = payload
        .as_array_mut()
        .context("collections payload was not a list")?;
    if let Some(folder_id) = folder_id {
        let collection = collections
            .iter_mut()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(collection_id))
            .context("collection no longer exists")?;
        let folders = collection
            .get_mut("folders")
            .and_then(Value::as_array_mut)
            .context("collection folders were not a list")?;
        move_matching(folders, folder_id, direction)?;
    } else {
        move_matching(collections, collection_id, direction)?;
    }
    auth.rpc_unit(
        "sync_push_collections",
        &json!({
            "p_profile_id": profile_id,
            "p_collections_json": payload,
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )?;
    serde_json::from_value(payload).context("updated collections did not match Nuvio's schema")
}

pub fn toggle_catalog(
    auth: &AuthService,
    profile_id: i32,
    collection_id: &str,
    folder_id: &str,
    source: CollectionCatalogSource,
) -> Result<Vec<Collection>> {
    mutate_folder(auth, profile_id, collection_id, folder_id, |folder| {
        let sources = normalized_sources(folder)?;
        let existing = sources
            .iter()
            .position(|item| addon_source_matches(item, &source));
        if let Some(index) = existing {
            sources.remove(index);
        } else {
            sources.push(json!({
                "provider": "addon",
                "addonId": source.addon_id,
                "type": source.content_type,
                "catalogId": source.catalog_id,
                "genre": source.genre,
            }));
        }
        sync_legacy_catalog_sources(folder)
    })
}

pub fn reorder_catalog(
    auth: &AuthService,
    profile_id: i32,
    collection_id: &str,
    folder_id: &str,
    source_index: usize,
    direction: i64,
) -> Result<Vec<Collection>> {
    anyhow::ensure!(
        matches!(direction, -1 | 1),
        "catalog direction must be -1 or 1"
    );
    mutate_folder(auth, profile_id, collection_id, folder_id, |folder| {
        let sources = normalized_sources(folder)?;
        anyhow::ensure!(source_index < sources.len(), "catalog no longer exists");
        let to = (source_index as i64 + direction).clamp(0, sources.len().saturating_sub(1) as i64)
            as usize;
        if source_index != to {
            sources.swap(source_index, to);
        }
        sync_legacy_catalog_sources(folder)
    })
}

fn mutate_folder(
    auth: &AuthService,
    profile_id: i32,
    collection_id: &str,
    folder_id: &str,
    mutation: impl FnOnce(&mut Value) -> Result<()>,
) -> Result<Vec<Collection>> {
    let mut payload = raw_payload(auth, profile_id)?;
    let collection = payload
        .as_array_mut()
        .context("collections payload was not a list")?
        .iter_mut()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(collection_id))
        .context("collection no longer exists")?;
    let folder = collection
        .get_mut("folders")
        .and_then(Value::as_array_mut)
        .and_then(|folders| {
            folders
                .iter_mut()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(folder_id))
        })
        .context("collection folder no longer exists")?;
    mutation(folder)?;
    push_payload(auth, profile_id, &payload)?;
    serde_json::from_value(payload).context("updated collections did not match Nuvio's schema")
}

fn normalized_sources(folder: &mut Value) -> Result<&mut Vec<Value>> {
    let needs_legacy = folder
        .get("sources")
        .and_then(Value::as_array)
        .map(|items| items.is_empty())
        .unwrap_or(true);
    if needs_legacy {
        let legacy = folder
            .get("catalogSources")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|mut item| {
                if let Some(object) = item.as_object_mut() {
                    object.insert("provider".to_string(), Value::String("addon".to_string()));
                }
                item
            })
            .collect();
        folder["sources"] = Value::Array(legacy);
    }
    folder
        .get_mut("sources")
        .and_then(Value::as_array_mut)
        .context("folder sources were not a list")
}

fn addon_source_matches(value: &Value, source: &CollectionCatalogSource) -> bool {
    !matches!(
        value.get("provider").and_then(Value::as_str),
        Some("tmdb" | "trakt")
    ) && value.get("addonId").and_then(Value::as_str) == Some(source.addon_id.as_str())
        && value.get("type").and_then(Value::as_str) == Some(source.content_type.as_str())
        && value.get("catalogId").and_then(Value::as_str) == Some(source.catalog_id.as_str())
}

fn sync_legacy_catalog_sources(folder: &mut Value) -> Result<()> {
    let catalogs = folder
        .get("sources")
        .and_then(Value::as_array)
        .context("folder sources were not a list")?
        .iter()
        .filter(|source| {
            !matches!(
                source.get("provider").and_then(Value::as_str),
                Some("tmdb" | "trakt")
            )
        })
        .filter_map(|source| {
            Some(json!({
                "addonId": source.get("addonId")?.as_str()?,
                "type": source.get("type")?.as_str()?,
                "catalogId": source.get("catalogId")?.as_str()?,
                "genre": source.get("genre").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect();
    folder["catalogSources"] = Value::Array(catalogs);
    Ok(())
}

fn push_payload(auth: &AuthService, profile_id: i32, payload: &Value) -> Result<()> {
    auth.rpc_unit(
        "sync_push_collections",
        &json!({
            "p_profile_id": profile_id,
            "p_collections_json": payload,
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )
}

fn raw_payload(auth: &AuthService, profile_id: i32) -> Result<Value> {
    let value = auth.rpc_value(
        "sync_pull_collections",
        &json!({ "p_profile_id": profile_id }),
    )?;
    let blob = value
        .as_array()
        .context("collections response was not a list")?
        .first();
    let Some(raw) = blob.and_then(|row| row.get("collections_json")) else {
        return Ok(json!([]));
    };
    if raw.is_null() {
        return Ok(json!([]));
    }
    let payload = match raw {
        Value::String(text) => {
            serde_json::from_str(text).context("collections JSON was invalid")?
        }
        value => value.clone(),
    };
    Ok(payload)
}

fn move_matching(items: &mut [Value], id: &str, direction: i64) -> Result<()> {
    let from = items
        .iter()
        .position(|item| item.get("id").and_then(Value::as_str) == Some(id))
        .context("collection item no longer exists")?;
    let to = (from as i64 + direction).clamp(0, items.len().saturating_sub(1) as i64) as usize;
    if from != to {
        items.swap(from, to);
    }
    Ok(())
}

fn default_provider() -> String {
    "addon".to_string()
}
fn default_tile_shape() -> String {
    "poster".to_string()
}
fn default_view_mode() -> String {
    "TABBED_GRID".to_string()
}
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_current_and_legacy_addon_sources() {
        let collections: Vec<Collection> = serde_json::from_value(json!([{
            "id": "favorites",
            "title": "Favorites",
            "folders": [{
                "id": "movies",
                "title": "Movies",
                "sources": [{ "provider": "addon", "addonId": "meta", "type": "movie", "catalogId": "popular" }]
            }, {
                "id": "shows",
                "title": "Shows",
                "catalogSources": [{ "addonId": "meta", "type": "series", "catalogId": "trending" }]
            }]
        }])).unwrap();
        assert_eq!(
            collections[0].folders[0].sources[0].catalog_id.as_deref(),
            Some("popular")
        );
        assert_eq!(
            collections[0].folders[1].catalog_sources[0].catalog_id,
            "trending"
        );
    }
}

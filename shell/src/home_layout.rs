//! Home layout organizer — the ordered list of catalogs *and* collections that
//! drives the home page, mirroring Nuvio's `HomeCatalogSettingsRepository` and
//! `HomeCatalogSettingsSyncService`.
//!
//! The wire contract has to stay byte-compatible with the Kotlin app, so a few
//! details are load-bearing:
//!
//!   * Preference keys are `"{manifest.id}:{type}:{catalogId}"` for catalogs and
//!     `"collection_{collectionId}"` for collections. Note the *manifest id* —
//!     the rest of this client keys catalogs by manifest URL, and pushing URLs
//!     would orphan every preference the phone wrote.
//!   * Preferences for addons this device cannot see are preserved verbatim and
//!     pushed back. Dropping them would wipe the phone's ordering for any addon
//!     that is not installed here.
//!   * The payload is merged over whatever the server already holds, so unknown
//!     top-level fields written by a newer Nuvio survive a desktop write.
//!   * `heroEnabled` / `heroSourceEnabled` are deliberately absent from the wire
//!     schema. Nuvio keeps them per-device, so we keep them in a local file.

use std::{
    collections::HashSet,
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::auth::AuthService;
use crate::collections::Collection;

pub const HERO_SOURCE_SELECTION_LIMIT: usize = 2;

const SHARED_SYNC_PLATFORM: &str = "home_catalog_shared";
const LEGACY_SYNC_PLATFORMS: [&str; 2] = ["mobile", "tv"];
const SHOW_CATALOG_TYPE_KEY: &str = "show_catalog_type";
const HIDE_UNRELEASED_CONTENT_KEY: &str = "hide_unreleased_content";
const COLLECTION_KEY_PREFIX: &str = "collection_";

// ---------------------------------------------------------------------------
// Wire schema — mirrors SyncCatalogItem / SyncHomeCatalogPayload exactly.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SyncCatalogItem {
    #[serde(rename = "addon_id", default)]
    pub addon_id: String,
    #[serde(rename = "type", default)]
    pub content_type: String,
    #[serde(rename = "catalog_id", default)]
    pub catalog_id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub order: i32,
    #[serde(rename = "custom_title", default)]
    pub custom_title: String,
    #[serde(rename = "is_collection", default)]
    pub is_collection: bool,
    #[serde(rename = "collection_id", default)]
    pub collection_id: String,
    #[serde(default)]
    pub key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SyncHomeCatalogPayload {
    #[serde(rename = "show_catalog_type", default = "default_true")]
    pub show_catalog_type: bool,
    #[serde(rename = "hide_unreleased_content", default)]
    pub hide_unreleased_content: bool,
    #[serde(default)]
    pub items: Vec<SyncCatalogItem>,
}

impl SyncCatalogItem {
    /// Mirrors Kotlin's `SyncCatalogItem.preferenceKey()`. The explicit `key`
    /// wins because addon ids can themselves contain colons, which makes the
    /// three-part decomposition ambiguous.
    fn preference_key(&self) -> String {
        if !self.key.trim().is_empty() {
            return self.key.clone();
        }
        if self.is_collection {
            format!("{COLLECTION_KEY_PREFIX}{}", self.collection_id)
        } else {
            format!("{}:{}:{}", self.addon_id, self.content_type, self.catalog_id)
        }
    }
}

// ---------------------------------------------------------------------------
// Definitions — what this device can actually render.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct CatalogDefinition {
    /// Nuvio preference key: `{manifest.id}:{type}:{catalogId}`.
    pub key: String,
    pub addon_id: String,
    pub content_type: String,
    pub catalog_id: String,
    pub catalog_name: String,
    pub addon_name: String,
}

impl CatalogDefinition {
    /// Mirrors the `home_catalog_default_title` resource: `"%1$s - %2$s"`.
    pub fn default_title(&self) -> String {
        format!(
            "{} - {}",
            self.catalog_name,
            media_type_label(&self.content_type)
        )
    }
}

#[derive(Clone, Debug)]
pub struct CollectionDefinition {
    pub key: String,
    pub collection_id: String,
    pub title: String,
    pub folder_count: usize,
    pub pinned_to_top: bool,
}

/// Mirrors `visibleCollectionsWithUniqueIds` + `buildCollectionDefinitions`.
pub fn build_collection_definitions(collections: &[Collection]) -> Vec<CollectionDefinition> {
    let mut seen = HashSet::new();
    collections
        .iter()
        .filter(|collection| !collection.folders.is_empty())
        .filter(|collection| seen.insert(collection.id.clone()))
        .map(|collection| CollectionDefinition {
            key: format!("{COLLECTION_KEY_PREFIX}{}", collection.id),
            collection_id: collection.id.clone(),
            title: collection.title.clone(),
            folder_count: collection.folders.len(),
            pinned_to_top: collection.pin_to_top,
        })
        .collect()
}

/// Mirrors `localizedMediaTypeLabel` (English strings).
pub fn media_type_label(content_type: &str) -> String {
    match content_type.trim().to_ascii_lowercase().as_str() {
        "movie" => "Movies".to_string(),
        "series" => "Series".to_string(),
        "anime" => "Anime".to_string(),
        "channel" => "Channels".to_string(),
        "tv" => "TV".to_string(),
        _ => {
            let mut chars = content_type.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Preferences
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Preference {
    key: String,
    custom_title: String,
    enabled: bool,
    hero_source_enabled: bool,
    order: i32,
}

/// Preferences are held in a `Vec` rather than a map so iteration order is
/// insertion order, matching the Kotlin `LinkedHashMap` the payload is built
/// from. Two clients that disagree on tie-breaking produce ordering churn.
#[derive(Clone, Debug, Default)]
pub struct HomeLayout {
    pub hero_enabled: bool,
    pub show_catalog_type: bool,
    pub hide_unreleased_content: bool,
    catalogs: Vec<CatalogDefinition>,
    collections: Vec<CollectionDefinition>,
    preferences: Vec<Preference>,
}

impl HomeLayout {
    fn index_of(&self, key: &str) -> Option<usize> {
        self.preferences.iter().position(|item| item.key == key)
    }

    fn preference(&self, key: &str) -> Option<&Preference> {
        self.preferences.iter().find(|item| item.key == key)
    }

    fn put(&mut self, preference: Preference) {
        match self.index_of(&preference.key) {
            Some(index) => self.preferences[index] = preference,
            None => self.preferences.push(preference),
        }
    }

    fn known_keys(&self) -> HashSet<String> {
        self.catalogs
            .iter()
            .map(|definition| definition.key.clone())
            .chain(
                self.collections
                    .iter()
                    .map(|definition| definition.key.clone()),
            )
            .collect()
    }

    /// Mirrors `allOrderedKeys()` — catalogs then collections, stably sorted by
    /// the stored order with unknown orders sinking to the bottom.
    fn all_ordered_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .catalogs
            .iter()
            .map(|definition| definition.key.clone())
            .chain(
                self.collections
                    .iter()
                    .map(|definition| definition.key.clone()),
            )
            .collect();
        keys.sort_by_key(|key| {
            self.preference(key)
                .map(|preference| preference.order)
                .unwrap_or(i32::MAX)
        });
        keys
    }

    /// Mirrors `normalizePreferences()`. Critically, preferences whose keys this
    /// device does not recognise are carried through untouched.
    fn normalize(&mut self) {
        let entries: Vec<(String, bool)> = self
            .catalogs
            .iter()
            .map(|definition| (definition.key.clone(), false))
            .chain(
                self.collections
                    .iter()
                    .map(|definition| (definition.key.clone(), true)),
            )
            .collect();
        let known: HashSet<String> = entries.iter().map(|(key, _)| key.clone()).collect();

        let mut next_order = self
            .preferences
            .iter()
            .map(|preference| preference.order)
            .max()
            .unwrap_or(-1)
            .saturating_add(1);

        // Sort by the stored order, falling back to the definition order for
        // entries the server has never seen.
        let mut ordered: Vec<(usize, i32, (String, bool))> = entries
            .into_iter()
            .enumerate()
            .map(|(default_index, entry)| {
                let order = self
                    .preference(&entry.0)
                    .map(|preference| preference.order)
                    .unwrap_or_else(|| next_order.saturating_add(default_index as i32));
                (default_index, order, entry)
            })
            .collect();
        ordered.sort_by_key(|(default_index, order, _)| (*order, *default_index));

        let mut normalized: Vec<Preference> = self
            .preferences
            .iter()
            .filter(|preference| !known.contains(&preference.key))
            .cloned()
            .collect();
        let mut hero_source_count = 0usize;

        for (_, _, (key, is_collection)) in ordered {
            let stored = self.preference(&key).cloned();
            let hero_source_enabled = if is_collection {
                false
            } else {
                stored
                    .as_ref()
                    .map(|preference| preference.hero_source_enabled)
                    .unwrap_or(true)
                    && hero_source_count < HERO_SOURCE_SELECTION_LIMIT
            };
            if hero_source_enabled {
                hero_source_count += 1;
            }
            let order = stored
                .as_ref()
                .map(|preference| preference.order)
                .unwrap_or_else(|| {
                    let assigned = next_order;
                    next_order = next_order.saturating_add(1);
                    assigned
                });
            normalized.push(Preference {
                key: key.clone(),
                custom_title: stored
                    .as_ref()
                    .map(|preference| preference.custom_title.clone())
                    .unwrap_or_default(),
                enabled: stored
                    .as_ref()
                    .map(|preference| preference.enabled)
                    .unwrap_or(true),
                hero_source_enabled,
                order,
            });
        }

        self.preferences = normalized;
    }

    /// Mirrors `enforcePinnedCollectionsAtTop()`.
    fn enforce_pinned_collections_at_top(&mut self) {
        let ordered_keys = self.all_ordered_keys();
        if ordered_keys.is_empty() {
            return;
        }
        let pinned: HashSet<&str> = self
            .collections
            .iter()
            .filter(|definition| definition.pinned_to_top)
            .map(|definition| definition.key.as_str())
            .collect();
        if pinned.is_empty() {
            return;
        }
        let (pinned_keys, rest): (Vec<String>, Vec<String>) = ordered_keys
            .iter()
            .cloned()
            .partition(|key| pinned.contains(key.as_str()));
        if pinned_keys.is_empty() {
            return;
        }
        let reordered: Vec<String> = pinned_keys.into_iter().chain(rest).collect();
        if reordered == ordered_keys {
            return;
        }
        self.assign_dense_order(&reordered);
    }

    fn assign_dense_order(&mut self, ordered_keys: &[String]) {
        for (index, key) in ordered_keys.iter().enumerate() {
            if let Some(position) = self.index_of(key) {
                self.preferences[position].order = index as i32;
            }
        }
    }

    // -- mutations ---------------------------------------------------------

    fn set_enabled(&mut self, key: &str, enabled: bool) -> Result<()> {
        let mut preference = self
            .preference(key)
            .cloned()
            .or_else(|| self.default_preference_for_missing_key(key))
            .context("that catalog or collection is no longer available")?;
        preference.enabled = enabled;
        self.put(preference);
        Ok(())
    }

    fn set_custom_title(&mut self, key: &str, title: &str) -> Result<()> {
        let mut preference = self
            .preference(key)
            .cloned()
            .or_else(|| self.default_preference_for_missing_key(key))
            .context("that catalog or collection is no longer available")?;
        preference.custom_title = title.trim().to_string();
        self.put(preference);
        Ok(())
    }

    /// Device-local; mirrors `setHeroSourceEnabled` including the silent no-op
    /// once the selection limit is reached.
    fn set_hero_source_enabled(&mut self, key: &str, enabled: bool) -> Result<()> {
        let mut preference = self
            .preference(key)
            .cloned()
            .or_else(|| self.default_preference_for_missing_key(key))
            .context("that catalog is no longer available")?;
        if !enabled {
            preference.hero_source_enabled = false;
        } else if self.selected_hero_source_count(Some(key)) >= HERO_SOURCE_SELECTION_LIMIT {
            return Ok(());
        } else {
            preference.hero_source_enabled = true;
        }
        self.put(preference);
        Ok(())
    }

    fn selected_hero_source_count(&self, excluding_key: Option<&str>) -> usize {
        let catalog_keys: HashSet<&str> = self
            .catalogs
            .iter()
            .map(|definition| definition.key.as_str())
            .collect();
        self.preferences
            .iter()
            .filter(|preference| {
                Some(preference.key.as_str()) != excluding_key
                    && catalog_keys.contains(preference.key.as_str())
                    && preference.hero_source_enabled
            })
            .count()
    }

    /// Mirrors `moveByIndex` — indices address the visible, ordered list.
    fn move_by_index(&mut self, from_index: usize, to_index: usize) -> Result<()> {
        let mut ordered_keys = self.all_ordered_keys();
        anyhow::ensure!(
            from_index < ordered_keys.len() && to_index < ordered_keys.len(),
            "that row is no longer in the home layout"
        );
        if from_index == to_index {
            return Ok(());
        }
        let key = ordered_keys.remove(from_index);
        ordered_keys.insert(to_index, key);
        self.assign_dense_order(&ordered_keys);
        Ok(())
    }

    /// Mirrors `resetToDefaults` — drops *this device's* preferences and lets
    /// normalization rebuild them from the installed addons and collections.
    /// Preferences for keys this device cannot see are dropped too, exactly as
    /// Nuvio does, because a reset is an explicit "start over" instruction.
    fn reset(&mut self) {
        self.hero_enabled = true;
        self.show_catalog_type = true;
        self.hide_unreleased_content = false;
        self.preferences.clear();
        self.normalize();
    }

    fn default_preference_for_missing_key(&self, key: &str) -> Option<Preference> {
        let is_catalog = self
            .catalogs
            .iter()
            .any(|definition| definition.key == key);
        let is_collection = self
            .collections
            .iter()
            .any(|definition| definition.key == key);
        if !is_catalog && !is_collection {
            return None;
        }
        Some(Preference {
            key: key.to_string(),
            custom_title: String::new(),
            enabled: true,
            hero_source_enabled: is_catalog
                && self.selected_hero_source_count(Some(key)) < HERO_SOURCE_SELECTION_LIMIT,
            order: self
                .preferences
                .iter()
                .map(|preference| preference.order)
                .max()
                .unwrap_or(-1)
                .saturating_add(1),
        })
    }

    // -- payload -----------------------------------------------------------

    /// Mirrors `exportToSyncPayload()`.
    fn export_to_sync_payload(&self) -> SyncHomeCatalogPayload {
        let mut sorted: Vec<&Preference> = self.preferences.iter().collect();
        sorted.sort_by_key(|preference| preference.order);

        let items = sorted
            .into_iter()
            .map(|preference| {
                let catalog = self
                    .catalogs
                    .iter()
                    .find(|definition| definition.key == preference.key);
                let collection = self
                    .collections
                    .iter()
                    .find(|definition| definition.key == preference.key);
                let is_collection =
                    collection.is_some() || preference.key.starts_with(COLLECTION_KEY_PREFIX);
                if is_collection {
                    SyncCatalogItem {
                        addon_id: String::new(),
                        content_type: String::new(),
                        catalog_id: String::new(),
                        enabled: preference.enabled,
                        order: preference.order,
                        custom_title: preference.custom_title.clone(),
                        is_collection: true,
                        collection_id: collection
                            .map(|definition| definition.collection_id.clone())
                            .unwrap_or_else(|| {
                                preference
                                    .key
                                    .strip_prefix(COLLECTION_KEY_PREFIX)
                                    .unwrap_or(&preference.key)
                                    .to_string()
                            }),
                        key: preference.key.clone(),
                    }
                } else {
                    let mut legacy = preference.key.splitn(3, ':');
                    let legacy_addon = legacy.next().unwrap_or_default().to_string();
                    let legacy_type = legacy.next().unwrap_or_default().to_string();
                    let legacy_catalog = legacy.next().unwrap_or_default().to_string();
                    SyncCatalogItem {
                        addon_id: catalog
                            .map(|definition| definition.addon_id.clone())
                            .unwrap_or(legacy_addon),
                        content_type: catalog
                            .map(|definition| definition.content_type.clone())
                            .unwrap_or(legacy_type),
                        catalog_id: catalog
                            .map(|definition| definition.catalog_id.clone())
                            .unwrap_or(legacy_catalog),
                        enabled: preference.enabled,
                        order: preference.order,
                        custom_title: preference.custom_title.clone(),
                        is_collection: false,
                        collection_id: String::new(),
                        key: preference.key.clone(),
                    }
                }
            })
            .collect();

        SyncHomeCatalogPayload {
            show_catalog_type: self.show_catalog_type,
            hide_unreleased_content: self.hide_unreleased_content,
            items,
        }
    }

    /// Mirrors `applyFromRemote()`.
    fn apply_from_remote(&mut self, payload: &SyncHomeCatalogPayload) {
        self.show_catalog_type = payload.show_catalog_type;
        self.hide_unreleased_content = payload.hide_unreleased_content;
        if payload.items.is_empty() {
            return;
        }

        let remote: Vec<Preference> = payload
            .items
            .iter()
            .map(|item| {
                let key = item.preference_key();
                let hero_source_enabled = self
                    .preference(&key)
                    .map(|preference| preference.hero_source_enabled)
                    .unwrap_or(true);
                Preference {
                    key,
                    custom_title: item.custom_title.clone(),
                    enabled: item.enabled,
                    hero_source_enabled,
                    order: item.order,
                }
            })
            .collect();

        let remote_keys: HashSet<&str> = remote.iter().map(|item| item.key.as_str()).collect();
        let known = self.known_keys();
        let preserved: Vec<Preference> = self
            .preferences
            .iter()
            .filter(|preference| {
                !remote_keys.contains(preference.key.as_str())
                    && (known.contains(&preference.key)
                        || requires_explicit_sync_key(&preference.key))
            })
            .cloned()
            .collect();

        self.preferences = preserved.into_iter().chain(remote).collect();
        self.normalize();
    }

    // -- projections -------------------------------------------------------

    pub fn ui_state(&self) -> HomeLayoutState {
        let mut items: Vec<HomeLayoutItem> = self
            .catalogs
            .iter()
            .map(|definition| {
                let preference = self.preference(&definition.key);
                let custom_title = preference
                    .map(|value| value.custom_title.clone())
                    .unwrap_or_default();
                let default_title = definition.default_title();
                HomeLayoutItem {
                    display_title: if custom_title.trim().is_empty() {
                        default_title.clone()
                    } else {
                        custom_title.clone()
                    },
                    key: definition.key.clone(),
                    default_title,
                    custom_title,
                    subtitle: definition.addon_name.clone(),
                    enabled: preference.map(|value| value.enabled).unwrap_or(true),
                    hero_source_enabled: preference
                        .map(|value| value.hero_source_enabled)
                        .unwrap_or(true),
                    order: preference.map(|value| value.order).unwrap_or(0),
                    is_collection: false,
                    collection_id: None,
                    pinned_to_top: false,
                }
            })
            .chain(self.collections.iter().map(|definition| {
                let preference = self.preference(&definition.key);
                let custom_title = preference
                    .map(|value| value.custom_title.clone())
                    .unwrap_or_default();
                HomeLayoutItem {
                    display_title: if custom_title.trim().is_empty() {
                        definition.title.clone()
                    } else {
                        custom_title.clone()
                    },
                    key: definition.key.clone(),
                    default_title: definition.title.clone(),
                    custom_title,
                    subtitle: format!(
                        "Collection • {} folder{}",
                        definition.folder_count,
                        if definition.folder_count == 1 { "" } else { "s" }
                    ),
                    enabled: preference.map(|value| value.enabled).unwrap_or(true),
                    hero_source_enabled: false,
                    order: preference.map(|value| value.order).unwrap_or(0),
                    is_collection: true,
                    collection_id: Some(definition.collection_id.clone()),
                    pinned_to_top: definition.pinned_to_top,
                }
            }))
            .collect();
        items.sort_by_key(|item| item.order);

        let known = self.known_keys();
        HomeLayoutState {
            hero_enabled: self.hero_enabled,
            show_catalog_type: self.show_catalog_type,
            hide_unreleased_content: self.hide_unreleased_content,
            hero_source_limit: HERO_SOURCE_SELECTION_LIMIT,
            preserved_count: self
                .preferences
                .iter()
                .filter(|preference| !known.contains(&preference.key))
                .count(),
            items,
        }
    }

    /// The subset the content service needs to order, filter and title rows.
    pub fn plan(&self) -> HomeLayoutPlan {
        HomeLayoutPlan {
            hero_enabled: self.hero_enabled,
            show_catalog_type: self.show_catalog_type,
            hide_unreleased_content: self.hide_unreleased_content,
            entries: self
                .preferences
                .iter()
                .map(|preference| HomeLayoutPlanEntry {
                    key: preference.key.clone(),
                    order: preference.order,
                    enabled: preference.enabled,
                    hero_source_enabled: preference.hero_source_enabled,
                    custom_title: preference.custom_title.clone(),
                })
                .collect(),
            rows: self
                .all_ordered_keys()
                .into_iter()
                .filter(|key| {
                    self.preference(key)
                        .map(|preference| preference.enabled)
                        .unwrap_or(true)
                })
                .map(|key| {
                    let collection = self
                        .collections
                        .iter()
                        .find(|definition| definition.key == key);
                    HomeLayoutRow {
                        collection_id: collection
                            .map(|definition| definition.collection_id.clone()),
                        is_collection: collection.is_some(),
                        key,
                    }
                })
                .collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Projections consumed by the webview and the content service
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeLayoutItem {
    pub key: String,
    pub default_title: String,
    pub display_title: String,
    pub custom_title: String,
    pub subtitle: String,
    pub enabled: bool,
    pub hero_source_enabled: bool,
    pub order: i32,
    pub is_collection: bool,
    pub collection_id: Option<String>,
    pub pinned_to_top: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeLayoutState {
    pub hero_enabled: bool,
    pub show_catalog_type: bool,
    pub hide_unreleased_content: bool,
    pub hero_source_limit: usize,
    /// Preferences kept for addons or collections this device cannot see. They
    /// are pushed back untouched so other devices keep their ordering.
    pub preserved_count: usize,
    pub items: Vec<HomeLayoutItem>,
}

#[derive(Clone, Debug)]
pub struct HomeLayoutPlanEntry {
    pub key: String,
    pub order: i32,
    pub enabled: bool,
    pub hero_source_enabled: bool,
    pub custom_title: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeLayoutRow {
    pub key: String,
    pub is_collection: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HomeLayoutPlan {
    pub hero_enabled: bool,
    pub show_catalog_type: bool,
    pub hide_unreleased_content: bool,
    pub entries: Vec<HomeLayoutPlanEntry>,
    pub rows: Vec<HomeLayoutRow>,
}

impl Default for HomeLayoutPlan {
    fn default() -> Self {
        Self {
            hero_enabled: true,
            show_catalog_type: true,
            hide_unreleased_content: false,
            entries: Vec::new(),
            rows: Vec::new(),
        }
    }
}

impl HomeLayoutPlan {
    pub fn entry(&self, key: &str) -> Option<&HomeLayoutPlanEntry> {
        self.entries.iter().find(|entry| entry.key == key)
    }

    pub fn order_of(&self, key: &str) -> i32 {
        self.entry(key).map(|entry| entry.order).unwrap_or(i32::MAX)
    }

    pub fn is_enabled(&self, key: &str) -> bool {
        self.entry(key).map(|entry| entry.enabled).unwrap_or(true)
    }

    pub fn is_hero_source(&self, key: &str) -> bool {
        self.entry(key)
            .map(|entry| entry.hero_source_enabled)
            .unwrap_or(true)
    }

    pub fn custom_title(&self, key: &str) -> Option<&str> {
        self.entry(key)
            .map(|entry| entry.custom_title.trim())
            .filter(|title| !title.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Server IO
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct RemoteRow {
    payload: SyncHomeCatalogPayload,
    settings_json: Map<String, Value>,
    updated_at: String,
}

impl RemoteRow {
    fn has(&self, key: &str) -> bool {
        self.settings_json.contains_key(key)
    }
}

/// `Err` means the server could not be reached — the caller must not treat that
/// as "there is nothing stored", or a push would wipe rows written elsewhere.
/// `Ok(None)` means the row genuinely does not exist or could not be parsed,
/// which is what Kotlin's per-platform `null` return covers.
fn fetch_remote_row(
    auth: &AuthService,
    profile_id: i32,
    platform: &str,
    local: &LocalState,
) -> Result<Option<RemoteRow>> {
    let response = auth
        .rpc_value(
            "sync_pull_home_catalog_settings",
            &json!({ "p_profile_id": profile_id, "p_platform": platform }),
        )
        .with_context(|| format!("could not read the {platform} home layout"))?;

    let Some(row) = response.as_array().and_then(|rows| rows.first()) else {
        return Ok(None);
    };
    let Some(settings) = row.get("settings_json") else {
        return Ok(None);
    };
    let settings = match settings {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        },
        value => value.clone(),
    };
    let Some(settings_json) = settings.as_object().cloned() else {
        return Ok(None);
    };

    // Mirrors `decodePayloadPreservingLocalDefaults`: a remote row that predates
    // a flag must not clobber what this device already believes.
    let Ok(mut payload) =
        serde_json::from_value::<SyncHomeCatalogPayload>(Value::Object(settings_json.clone()))
    else {
        return Ok(None);
    };
    if !settings_json.contains_key(SHOW_CATALOG_TYPE_KEY) {
        payload.show_catalog_type = local.show_catalog_type;
    }
    if !settings_json.contains_key(HIDE_UNRELEASED_CONTENT_KEY) {
        payload.hide_unreleased_content = local.hide_unreleased_content;
    }

    Ok(Some(RemoteRow {
        payload,
        settings_json,
        updated_at: row
            .get("updated_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }))
}

/// Kotlin's `maxByOrNull` keeps the *first* maximum; `Iterator::max_by_key`
/// keeps the last. Tie-breaking differently would pick a different row when two
/// platforms share a timestamp.
fn newest<'a>(rows: impl IntoIterator<Item = &'a RemoteRow>) -> Option<&'a RemoteRow> {
    let mut best: Option<&RemoteRow> = None;
    for row in rows {
        if best.is_none_or(|current| row.updated_at > current.updated_at) {
            best = Some(row);
        }
    }
    best
}

/// Mirrors `fetchBestRemotePayload` + `withNewestStandaloneSettings`.
fn fetch_best_remote_payload(
    auth: &AuthService,
    profile_id: i32,
    local: &LocalState,
) -> Result<Option<SyncHomeCatalogPayload>> {
    let shared = fetch_remote_row(auth, profile_id, SHARED_SYNC_PLATFORM, local)?;
    let mut legacy = Vec::new();
    for platform in LEGACY_SYNC_PLATFORMS {
        if let Some(row) = fetch_remote_row(auth, profile_id, platform, local)? {
            legacy.push(row);
        }
    }

    let rows: Vec<&RemoteRow> = shared.iter().chain(legacy.iter()).collect();
    let Some(selected) = newest(
        rows.iter()
            .copied()
            .filter(|row| !row.payload.items.is_empty()),
    )
    .or(shared.as_ref())
    .or_else(|| newest(legacy.iter())) else {
        return Ok(None);
    };

    let mut payload = selected.payload.clone();
    if let Some(row) = newest(
        rows.iter()
            .copied()
            .filter(|row| row.has(SHOW_CATALOG_TYPE_KEY)),
    ) {
        payload.show_catalog_type = row.payload.show_catalog_type;
    }
    if let Some(row) = newest(
        rows.iter()
            .copied()
            .filter(|row| row.has(HIDE_UNRELEASED_CONTENT_KEY)),
    ) {
        payload.hide_unreleased_content = row.payload.hide_unreleased_content;
    }
    Ok(Some(payload))
}

/// Mirrors `pushToRemote` + `mergedSharedPayloadJson`. Local keys win, remote
/// keys we do not model survive.
fn push_to_remote(
    auth: &AuthService,
    profile_id: i32,
    payload: &SyncHomeCatalogPayload,
    local: &LocalState,
) -> Result<()> {
    let local_json = serde_json::to_value(payload)?;
    let local_object = local_json
        .as_object()
        .context("home layout payload did not serialize to an object")?;

    // A failed read here must abort the write: merging against an empty object
    // would silently drop any field a newer Nuvio wrote.
    let mut merged = fetch_remote_row(auth, profile_id, SHARED_SYNC_PLATFORM, local)?
        .map(|row| row.settings_json)
        .unwrap_or_default();
    for (key, value) in local_object {
        merged.insert(key.clone(), value.clone());
    }

    auth.rpc_unit(
        "sync_push_home_catalog_settings",
        &json!({
            "p_profile_id": profile_id,
            "p_platform": SHARED_SYNC_PLATFORM,
            "p_settings_json": Value::Object(merged),
            "p_origin_client_id": auth.sync_client_id(),
        }),
    )
}

/// Kotlin's `String.requiresExplicitSyncKey()`.
fn requires_explicit_sync_key(key: &str) -> bool {
    !key.starts_with(COLLECTION_KEY_PREFIX) && key.matches(':').count() > 2
}

// ---------------------------------------------------------------------------
// Device-local state (hero selection is never synced by Nuvio)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LocalState {
    #[serde(default = "default_true")]
    hero_enabled: bool,
    #[serde(default = "default_true")]
    show_catalog_type: bool,
    #[serde(default)]
    hide_unreleased_content: bool,
    /// Explicit per-catalog hero overrides. Absent means "default to on", which
    /// normalization then caps at `HERO_SOURCE_SELECTION_LIMIT`.
    #[serde(default)]
    hero_sources: Vec<(String, bool)>,
}

impl Default for LocalState {
    fn default() -> Self {
        Self {
            hero_enabled: true,
            show_catalog_type: true,
            hide_unreleased_content: false,
            hero_sources: Vec::new(),
        }
    }
}

fn local_state_path() -> Option<PathBuf> {
    let base = env::var_os("LOCALAPPDATA")
        .or_else(|| env::var_os("XDG_CONFIG_HOME"))
        .or_else(|| env::var_os("HOME"))?;
    Some(
        PathBuf::from(base)
            .join("Nuvio")
            .join("rust-webview-poc")
            .join("home_layout_local.json"),
    )
}

fn local_scope(auth: &AuthService, profile_id: i32) -> String {
    let user = auth.snapshot().user_id.unwrap_or_default();
    format!("{user}:{profile_id}")
}

fn load_local_document() -> Map<String, Value> {
    local_state_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn load_local_state(auth: &AuthService, profile_id: i32) -> LocalState {
    load_local_document()
        .get(&local_scope(auth, profile_id))
        .cloned()
        .and_then(|value| serde_json::from_value::<LocalState>(value).ok())
        .unwrap_or_default()
}

fn save_local_state(auth: &AuthService, profile_id: i32, state: &LocalState) {
    let Some(path) = local_state_path() else {
        return;
    };
    let mut document = load_local_document();
    let Ok(value) = serde_json::to_value(state) else {
        return;
    };
    document.insert(local_scope(auth, profile_id), value);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&Value::Object(document)) {
        let _ = fs::write(path, text);
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Builds the layout for this profile: local device state, then the server's
/// ordering applied on top.
pub fn load(
    auth: &AuthService,
    profile_id: i32,
    catalogs: Vec<CatalogDefinition>,
    collections: &[Collection],
) -> Result<HomeLayout> {
    let local = load_local_state(auth, profile_id);
    let mut layout = HomeLayout {
        hero_enabled: local.hero_enabled,
        show_catalog_type: local.show_catalog_type,
        hide_unreleased_content: local.hide_unreleased_content,
        catalogs,
        collections: build_collection_definitions(collections),
        preferences: Vec::new(),
    };

    if let Some(payload) = fetch_best_remote_payload(auth, profile_id, &local)? {
        layout.apply_from_remote(&payload);
    }
    layout.normalize();

    // Hero overrides are layered on afterwards rather than seeded as
    // preferences: they carry no order of their own, and feeding a placeholder
    // order into normalization would poison the counter every later row uses.
    for (key, hero_source_enabled) in &local.hero_sources {
        if let Some(index) = layout.index_of(key) {
            layout.preferences[index].hero_source_enabled = *hero_source_enabled;
        }
    }
    layout.normalize();
    layout.enforce_pinned_collections_at_top();
    Ok(layout)
}

/// Applies a mutation and pushes it, re-reading the server first so a change
/// made on the phone a moment ago is not silently overwritten.
fn mutate(
    auth: &AuthService,
    profile_id: i32,
    catalogs: Vec<CatalogDefinition>,
    collections: &[Collection],
    mutation: impl FnOnce(&mut HomeLayout) -> Result<()>,
) -> Result<HomeLayout> {
    let mut layout = load(auth, profile_id, catalogs, collections)?;
    let had_remote_items = !layout.preferences.is_empty();
    mutation(&mut layout)?;
    layout.normalize();
    layout.enforce_pinned_collections_at_top();

    let payload = layout.export_to_sync_payload();
    anyhow::ensure!(
        !payload.items.is_empty() || !had_remote_items,
        "refusing to push an empty home layout over existing synced data"
    );

    let local = local_state_of(&layout);
    save_local_state(auth, profile_id, &local);

    push_to_remote(auth, profile_id, &payload, &local)?;
    Ok(layout)
}

/// Snapshot of the device-local half of the layout, rebuilt from whatever the
/// mutation left in place.
fn local_state_of(layout: &HomeLayout) -> LocalState {
    LocalState {
        hero_enabled: layout.hero_enabled,
        show_catalog_type: layout.show_catalog_type,
        hide_unreleased_content: layout.hide_unreleased_content,
        hero_sources: layout
            .preferences
            .iter()
            .filter(|preference| !preference.hero_source_enabled)
            .map(|preference| (preference.key.clone(), false))
            .collect(),
    }
}

pub enum Mutation<'a> {
    SetEnabled { key: &'a str, enabled: bool },
    SetCustomTitle { key: &'a str, title: &'a str },
    SetHeroSourceEnabled { key: &'a str, enabled: bool },
    SetHeroEnabled(bool),
    SetShowCatalogType(bool),
    SetHideUnreleasedContent(bool),
    Move { from: usize, to: usize },
    Reset,
}

pub fn apply(
    auth: &AuthService,
    profile_id: i32,
    catalogs: Vec<CatalogDefinition>,
    collections: &[Collection],
    mutation: Mutation<'_>,
) -> Result<HomeLayout> {
    // Hero preferences never leave the device, so they skip the server round
    // trip entirely — matching Nuvio, which does not call triggerPush for them.
    if let Mutation::SetHeroEnabled(_) | Mutation::SetHeroSourceEnabled { .. } = mutation {
        let mut layout = load(auth, profile_id, catalogs, collections)?;
        match mutation {
            Mutation::SetHeroEnabled(enabled) => layout.hero_enabled = enabled,
            Mutation::SetHeroSourceEnabled { key, enabled } => {
                layout.set_hero_source_enabled(key, enabled)?
            }
            _ => unreachable!(),
        }
        save_local_state(auth, profile_id, &local_state_of(&layout));
        return Ok(layout);
    }

    mutate(auth, profile_id, catalogs, collections, |layout| {
        match mutation {
            Mutation::SetEnabled { key, enabled } => layout.set_enabled(key, enabled),
            Mutation::SetCustomTitle { key, title } => layout.set_custom_title(key, title),
            Mutation::SetShowCatalogType(enabled) => {
                layout.show_catalog_type = enabled;
                Ok(())
            }
            Mutation::SetHideUnreleasedContent(enabled) => {
                layout.hide_unreleased_content = enabled;
                Ok(())
            }
            Mutation::Move { from, to } => layout.move_by_index(from, to),
            Mutation::Reset => {
                layout.reset();
                Ok(())
            }
            Mutation::SetHeroEnabled(_) | Mutation::SetHeroSourceEnabled { .. } => unreachable!(),
        }
    })
}

// ---------------------------------------------------------------------------
// Unreleased filtering (mirrors ReleaseInfoUtils.isUnreleased)
// ---------------------------------------------------------------------------

/// `true` when the title has not come out yet. Dates are compared in UTC; Nuvio
/// uses the device timezone, so a title can differ for a few hours around its
/// release day.
pub fn is_unreleased(released: Option<&str>, release_info: Option<&str>) -> bool {
    let today = today_epoch_day();
    if let Some(day) = released
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(parse_epoch_day)
    {
        return day > today;
    }
    let Some(info) = release_info.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    if let Some(day) = parse_epoch_day(info) {
        return day > today;
    }
    let Some(year) = first_year(info) else {
        return false;
    };
    year > epoch_day_to_year(today)
}

/// Accepts `YYYY-MM-DD` and any ISO 8601 timestamp that starts with one.
fn parse_epoch_day(value: &str) -> Option<i64> {
    let date = value.get(..10)?;
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = date.get(0..4)?.parse().ok()?;
    let month: i64 = date.get(5..7)?.parse().ok()?;
    let day: i64 = date.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(civil_to_epoch_day(year, month, day))
}

/// Kotlin's `Regex("""\b(19|20)\d{2}\b""")` — only 19xx/20xx count, and the run
/// of digits has to be exactly four long.
fn first_year(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    (0..bytes.len().saturating_sub(3)).find_map(|start| {
        let end = start + 4;
        if start > 0 && is_word(bytes[start - 1]) {
            return None;
        }
        if end < bytes.len() && is_word(bytes[end]) {
            return None;
        }
        let text = std::str::from_utf8(&bytes[start..end]).ok()?;
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let year: i64 = text.parse().ok()?;
        (1900..=2099).contains(&year).then_some(year)
    })
}

/// Howard Hinnant's `days_from_civil`.
fn civil_to_epoch_day(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn today_epoch_day() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64 / 86_400)
        .unwrap_or_default()
}

fn epoch_day_to_year(epoch_day: i64) -> i64 {
    let z = epoch_day + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    if month_prime >= 10 { year + 1 } else { year }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(addon: &str, content_type: &str, id: &str) -> CatalogDefinition {
        CatalogDefinition {
            key: format!("{addon}:{content_type}:{id}"),
            addon_id: addon.to_string(),
            content_type: content_type.to_string(),
            catalog_id: id.to_string(),
            catalog_name: id.to_string(),
            addon_name: addon.to_string(),
        }
    }

    fn layout(catalogs: Vec<CatalogDefinition>) -> HomeLayout {
        let mut layout = HomeLayout {
            hero_enabled: true,
            show_catalog_type: true,
            catalogs,
            ..HomeLayout::default()
        };
        layout.normalize();
        layout
    }

    #[test]
    fn preference_keys_use_the_manifest_id_not_the_url() {
        let definition = catalog("com.example.meta", "movie", "popular");
        assert_eq!(definition.key, "com.example.meta:movie:popular");
        assert_eq!(definition.default_title(), "popular - Movies");
    }

    #[test]
    fn unknown_keys_survive_a_desktop_write() {
        let mut layout = layout(vec![catalog("local", "movie", "top")]);
        layout.apply_from_remote(&SyncHomeCatalogPayload {
            show_catalog_type: true,
            hide_unreleased_content: false,
            items: vec![
                SyncCatalogItem {
                    addon_id: "phone.only".to_string(),
                    content_type: "series".to_string(),
                    catalog_id: "trending".to_string(),
                    enabled: true,
                    order: 0,
                    custom_title: "Phone Row".to_string(),
                    is_collection: false,
                    collection_id: String::new(),
                    key: "phone.only:series:trending".to_string(),
                },
                SyncCatalogItem {
                    addon_id: "local".to_string(),
                    content_type: "movie".to_string(),
                    catalog_id: "top".to_string(),
                    enabled: true,
                    order: 1,
                    custom_title: String::new(),
                    is_collection: false,
                    collection_id: String::new(),
                    key: "local:movie:top".to_string(),
                },
            ],
        });

        let payload = layout.export_to_sync_payload();
        let phone = payload
            .items
            .iter()
            .find(|item| item.key == "phone.only:series:trending")
            .expect("preference for an addon this device lacks must be preserved");
        assert_eq!(phone.custom_title, "Phone Row");
        assert_eq!(phone.addon_id, "phone.only");
        assert_eq!(phone.catalog_id, "trending");
    }

    #[test]
    fn hero_sources_are_capped_and_never_serialized() {
        let mut layout = layout(vec![
            catalog("a", "movie", "one"),
            catalog("a", "movie", "two"),
            catalog("a", "movie", "three"),
        ]);
        layout.normalize();
        assert_eq!(layout.selected_hero_source_count(None), 2);

        let json = serde_json::to_value(layout.export_to_sync_payload()).unwrap();
        let item = &json["items"][0];
        assert!(item.get("hero_source_enabled").is_none());
        assert!(item.get("heroSourceEnabled").is_none());
    }

    #[test]
    fn moving_a_row_writes_a_dense_order() {
        let mut layout = layout(vec![
            catalog("a", "movie", "one"),
            catalog("a", "movie", "two"),
            catalog("a", "movie", "three"),
        ]);
        layout.move_by_index(2, 0).unwrap();
        let keys = layout.all_ordered_keys();
        assert_eq!(keys[0], "a:movie:three");
        assert_eq!(layout.preference("a:movie:three").unwrap().order, 0);
        assert_eq!(layout.preference("a:movie:one").unwrap().order, 1);
    }

    #[test]
    fn collections_export_with_their_collection_id() {
        let mut layout = HomeLayout {
            collections: vec![CollectionDefinition {
                key: "collection_abc".to_string(),
                collection_id: "abc".to_string(),
                title: "Streaming Platforms".to_string(),
                folder_count: 10,
                pinned_to_top: false,
            }],
            ..HomeLayout::default()
        };
        layout.normalize();
        let payload = layout.export_to_sync_payload();
        let item = &payload.items[0];
        assert!(item.is_collection);
        assert_eq!(item.collection_id, "abc");
        assert_eq!(item.key, "collection_abc");
        assert!(item.addon_id.is_empty());
    }

    #[test]
    fn pinned_collections_are_forced_to_the_top() {
        let mut layout = HomeLayout {
            catalogs: vec![catalog("a", "movie", "one")],
            collections: vec![CollectionDefinition {
                key: "collection_pinned".to_string(),
                collection_id: "pinned".to_string(),
                title: "Pinned".to_string(),
                folder_count: 1,
                pinned_to_top: true,
            }],
            ..HomeLayout::default()
        };
        layout.normalize();
        layout.enforce_pinned_collections_at_top();
        assert_eq!(layout.all_ordered_keys()[0], "collection_pinned");
    }

    #[test]
    fn explicit_sync_keys_are_required_for_colon_heavy_addon_ids() {
        assert!(requires_explicit_sync_key("https://host/x:movie:top"));
        assert!(!requires_explicit_sync_key("com.example:movie:top"));
        assert!(!requires_explicit_sync_key("collection_abc"));
    }

    #[test]
    fn blank_keys_fall_back_to_the_three_part_decomposition() {
        let item = SyncCatalogItem {
            addon_id: "com.example".to_string(),
            content_type: "movie".to_string(),
            catalog_id: "top".to_string(),
            enabled: true,
            order: 0,
            custom_title: String::new(),
            is_collection: false,
            collection_id: String::new(),
            key: String::new(),
        };
        assert_eq!(item.preference_key(), "com.example:movie:top");
    }

    #[test]
    fn hero_overrides_do_not_poison_the_order_counter() {
        // Regression: seeding a hero-only preference with a placeholder order
        // pushed `next_order` to i32::MAX, collapsing every new row onto it.
        let mut layout = layout(vec![catalog("a", "movie", "one")]);
        layout.preferences[0].hero_source_enabled = false;
        layout.catalogs.push(catalog("a", "movie", "two"));
        layout.normalize();

        let orders: Vec<i32> = layout
            .preferences
            .iter()
            .map(|preference| preference.order)
            .collect();
        assert_eq!(orders, vec![0, 1]);
    }

    #[test]
    fn a_reordered_layout_keeps_distinct_orders() {
        let mut layout = layout(vec![
            catalog("a", "movie", "one"),
            catalog("a", "movie", "two"),
            catalog("a", "series", "three"),
        ]);
        layout.move_by_index(0, 2).unwrap();
        layout.normalize();
        let mut orders: Vec<i32> = layout
            .preferences
            .iter()
            .map(|preference| preference.order)
            .collect();
        orders.sort_unstable();
        assert_eq!(orders, vec![0, 1, 2]);
    }

    #[test]
    fn release_dates_decide_unreleased_titles() {
        assert!(is_unreleased(Some("2099-01-01"), None));
        assert!(is_unreleased(Some("2099-01-01T20:00:00.000Z"), None));
        assert!(!is_unreleased(Some("1999-01-01"), None));
        assert!(is_unreleased(None, Some("2099")));
        assert!(!is_unreleased(None, Some("2001")));
        assert!(!is_unreleased(None, None));
        // Nuvio's year regex only recognises 19xx/20xx, so anything else is
        // treated as released rather than guessed at.
        assert!(!is_unreleased(None, Some("2999")));
        assert!(!is_unreleased(None, Some("Season 12099")));
        assert_eq!(civil_to_epoch_day(1970, 1, 1), 0);
        assert_eq!(civil_to_epoch_day(2000, 3, 1), 11017);
        assert_eq!(epoch_day_to_year(0), 1970);
    }
}

use std::{
    collections::HashSet,
    io::Read,
    net::{IpAddr, ToSocketAddrs},
    sync::OnceLock,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::auth::AddonRow;
use crate::home_layout::{CatalogDefinition, HomeLayoutPlan, HomeLayoutRow, media_type_label};

/// Nuvio fetches home catalogs in batches rather than all at once; this client
/// fetches in parallel, so it caps the fan-out instead.
const HOME_CATALOG_LIMIT: usize = 24;
const HOME_HERO_ITEM_LIMIT: usize = 8;
const COLLECTION_CATALOG_LIMIT: usize = 60;
const MAX_ADDON_JSON_BYTES: usize = 8 * 1024 * 1024;
const ADDON_WORKERS: usize = 8;

fn content_pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(ADDON_WORKERS)
            .thread_name(|index| format!("nuvio-addon-{index}"))
            .build()
            .expect("valid addon worker pool")
    })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum Resource {
    Name(String),
    Detailed {
        name: String,
        #[serde(default)]
        types: Vec<String>,
        #[serde(default, rename = "idPrefixes")]
        id_prefixes: Vec<String>,
    },
}

#[derive(Clone, Debug, Deserialize)]
struct CatalogExtra {
    name: String,
    #[serde(default, rename = "isRequired")]
    is_required: bool,
    #[serde(default)]
    options: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ManifestCatalog {
    id: String,
    #[serde(rename = "type")]
    content_type: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    extra: Vec<CatalogExtra>,
}

#[derive(Clone, Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    logo: Option<String>,
    #[serde(default)]
    types: Vec<String>,
    #[serde(default, rename = "idPrefixes")]
    id_prefixes: Vec<String>,
    #[serde(default)]
    resources: Vec<Resource>,
    #[serde(default)]
    catalogs: Vec<ManifestCatalog>,
    #[serde(default, rename = "behaviorHints")]
    behavior_hints: ManifestBehaviorHints,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestBehaviorHints {
    #[serde(default)]
    configurable: bool,
    #[serde(default)]
    configuration_required: bool,
}

#[derive(Clone, Debug)]
struct InstalledManifest {
    url: String,
    display_name: String,
    manifest: Manifest,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMeta {
    #[serde(default)]
    pub id: String,
    #[serde(
        default,
        rename(deserialize = "type", serialize = "contentType"),
        alias = "contentType"
    )]
    pub content_type: String,
    #[serde(default)]
    pub name: String,
    pub poster: Option<String>,
    pub background: Option<String>,
    pub banner: Option<String>,
    pub logo: Option<String>,
    #[serde(default, rename = "posterShape")]
    pub poster_shape: Option<String>,
    pub description: Option<String>,
    #[serde(default, rename = "releaseInfo")]
    pub release_info: Option<String>,
    pub released: Option<String>,
    #[serde(default, rename = "imdbRating")]
    pub imdb_rating: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub cast: Vec<MetaPerson>,
    #[serde(default)]
    pub director: Vec<String>,
    #[serde(default)]
    pub writer: Vec<String>,
    pub status: Option<String>,
    #[serde(default, rename = "ageRating")]
    pub age_rating: Option<String>,
    #[serde(default, rename = "lastAirDate")]
    pub last_air_date: Option<String>,
    pub country: Option<String>,
    pub awards: Option<String>,
    pub language: Option<String>,
    pub website: Option<String>,
    #[serde(default)]
    pub trailers: Vec<MetaTrailer>,
    #[serde(default)]
    pub external_ratings: Vec<ExternalRating>,
    #[serde(default, rename = "defaultVideoId")]
    pub default_video_id: Option<String>,
    #[serde(default, rename = "hasScheduledVideos")]
    pub has_scheduled_videos: bool,
    #[serde(default)]
    pub videos: Vec<Video>,
    #[serde(default)]
    pub source_manifest_url: String,
    #[serde(default)]
    pub addon_name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetaPerson {
    #[serde(default)]
    pub name: String,
    pub role: Option<String>,
    pub photo: Option<String>,
    pub tmdb_id: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaTrailer {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub site: String,
    #[serde(default, rename = "type")]
    pub trailer_type: String,
    pub official: Option<bool>,
    pub published_at: Option<String>,
    pub season_number: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRating {
    pub source: String,
    pub value: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub season: Option<i64>,
    #[serde(default)]
    pub episode: Option<i64>,
    #[serde(default)]
    pub released: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default, rename = "seasonPoster")]
    pub season_poster: Option<String>,
    pub overview: Option<String>,
    pub runtime: Option<i64>,
    #[serde(default = "default_true")]
    pub available: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSection {
    pub key: String,
    /// Nuvio home-layout key (`{manifest.id}:{type}:{catalogId}`). Distinct from
    /// `key`, which is URL-based because this client fetches by manifest URL.
    pub pref_key: String,
    pub title: String,
    pub subtitle: String,
    pub manifest_url: String,
    pub content_type: String,
    pub catalog_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    pub items: Vec<ContentMeta>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomePayload {
    pub sections: Vec<CatalogSection>,
    /// Catalogs and collections interleaved in the user's configured order, the
    /// way Nuvio's home screen walks `HomeCatalogSettingsItem`s.
    pub rows: Vec<HomeLayoutRow>,
    pub hero: Option<ContentMeta>,
    pub hero_items: Vec<ContentMeta>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonDescriptor {
    pub url: String,
    pub name: String,
    /// Manifest `version`. Empty when the addon is unreachable, since the
    /// manifest is fetched at runtime and never stored in the synced row.
    pub version: String,
    pub enabled: bool,
    pub sort_order: i32,
    pub configurable: bool,
    pub configuration_required: bool,
    pub configure_url: Option<String>,
    pub catalog_count: usize,
    pub resource_names: Vec<String>,
    pub logo: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverCatalog {
    pub key: String,
    pub addon_name: String,
    pub manifest_url: String,
    pub content_type: String,
    pub catalog_id: String,
    pub catalog_name: String,
    pub genre_options: Vec<String>,
    pub genre_required: bool,
    pub supports_pagination: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableCollectionCatalog {
    pub addon_id: String,
    pub addon_name: String,
    pub content_type: String,
    pub catalog_id: String,
    pub catalog_name: String,
    pub genre_options: Vec<String>,
    pub genre_required: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSource {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub url: Option<String>,
    #[serde(default, rename = "infoHash")]
    pub info_hash: Option<String>,
    #[serde(default, rename = "fileIdx")]
    pub file_idx: Option<i64>,
    #[serde(default, rename = "externalUrl")]
    pub external_url: Option<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub addon_name: String,
    #[serde(default)]
    pub addon_id: String,
    #[serde(default, rename = "type")]
    pub stream_type: Option<String>,
    #[serde(default, rename = "behaviorHints")]
    pub behavior_hints: StreamBehaviorHints,
    #[serde(default)]
    pub addon_logo: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamBehaviorHints {
    pub binge_group: Option<String>,
    pub video_hash: Option<String>,
    pub video_size: Option<i64>,
    pub filename: Option<String>,
    #[serde(default)]
    pub not_web_ready: bool,
    #[serde(default, rename = "proxyHeaders")]
    pub proxy_headers: StreamProxyHeaders,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamProxyHeaders {
    #[serde(default)]
    pub request: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub response: std::collections::HashMap<String, String>,
}

#[derive(Clone)]
pub struct ContentService {
    client: Client,
    manifests: Vec<InstalledManifest>,
    addon_signature: String,
}

impl Default for ContentService {
    fn default() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                // Redirects are intentionally resolved by the addon author in
                // its manifest URL. Following them here could turn a public
                // addon into a request to a private/local service.
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("NuvioRustPoc/0.2.0")
                .build()
                .expect("valid content HTTP client"),
            manifests: Vec::new(),
            addon_signature: String::new(),
        }
    }
}

impl ContentService {
    pub fn invalidate(&mut self) {
        self.manifests.clear();
        self.addon_signature.clear();
    }

    pub fn snapshot(&mut self, addons: &[AddonRow]) -> Self {
        self.ensure_manifests(addons);
        self.clone()
    }

    pub fn http_client(&self) -> Client {
        self.client.clone()
    }

    pub fn resolve_meta_canonical(
        &mut self,
        addons: &[AddonRow],
        content_type: &str,
        id: &str,
        config: &crate::metadata::MetadataConfig,
    ) -> Result<ContentMeta> {
        let lookup_id =
            crate::metadata::addon_lookup_id(&self.client, content_type, id, &config.tmdb);
        match self.resolve_meta(addons, content_type, &lookup_id) {
            Ok(meta) => Ok(crate::metadata::enrich_tmdb(&self.client, meta, config)),
            Err(_) => crate::metadata::standalone(&self.client, content_type, id, config),
        }
    }

    pub fn inspect_addon(&self, raw_url: &str) -> Result<(String, String)> {
        let url = normalize_manifest_url(raw_url)?;
        let manifest: Manifest = get_json(&self.client, &url)?;
        if manifest.id.trim().is_empty() || manifest.name.trim().is_empty() {
            bail!("addon manifest is missing an id or name");
        }
        Ok((url, manifest.name))
    }

    pub fn addon_descriptors(&mut self, addons: &[AddonRow]) -> Vec<AddonDescriptor> {
        let errors = self.ensure_manifests(addons);
        addons
            .iter()
            .map(|addon| self.describe(addon, &errors))
            .collect()
    }

    fn describe(&self, addon: &AddonRow, errors: &[String]) -> AddonDescriptor {
        let normalized = normalize_manifest_url(&addon.url).unwrap_or_else(|_| addon.url.clone());
        let installed = self.manifests.iter().find(|item| item.url == normalized);
        let error = if installed.is_none() {
            errors
                .iter()
                .find(|message| message.contains(addon.name.as_deref().unwrap_or(&addon.url)))
                .cloned()
        } else {
            None
        };
        AddonDescriptor {
            url: normalized.clone(),
            name: addon
                .name
                .clone()
                .filter(|name| !name.trim().is_empty())
                .or_else(|| installed.map(|item| item.display_name.clone()))
                .unwrap_or_else(|| "Stremio addon".to_string()),
            version: installed
                .map(|item| item.manifest.version.clone())
                .unwrap_or_default(),
            enabled: addon.enabled,
            sort_order: addon.sort_order,
            configurable: installed
                .map(|item| item.manifest.behavior_hints.configurable)
                .unwrap_or(false),
            configuration_required: installed
                .map(|item| item.manifest.behavior_hints.configuration_required)
                .unwrap_or(false),
            configure_url: installed
                .filter(|item| {
                    item.manifest.behavior_hints.configurable
                        || item.manifest.behavior_hints.configuration_required
                })
                .map(|_| configure_url(&normalized)),
            catalog_count: installed
                .map(|item| item.manifest.catalogs.len())
                .unwrap_or(0),
            resource_names: installed
                .map(|item| {
                    item.manifest
                        .resources
                        .iter()
                        .map(|resource| match resource {
                            Resource::Name(name) => name.clone(),
                            Resource::Detailed { name, .. } => name.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            logo: installed
                .and_then(|item| item.manifest.logo.as_deref())
                .and_then(|logo| resolve_asset_url(&normalized, logo)),
            error,
        }
    }

    pub fn available_collection_catalogs(
        &mut self,
        addons: &[AddonRow],
    ) -> Vec<AvailableCollectionCatalog> {
        self.ensure_manifests(addons);
        self.manifests
            .iter()
            .flat_map(|installed| {
                installed
                    .manifest
                    .catalogs
                    .iter()
                    .filter_map(move |catalog| {
                        if catalog
                            .extra
                            .iter()
                            .any(|extra| extra.is_required && extra.name != "genre")
                        {
                            return None;
                        }
                        let genre = catalog.extra.iter().find(|extra| extra.name == "genre");
                        Some(AvailableCollectionCatalog {
                            addon_id: installed.manifest.id.clone(),
                            addon_name: installed.display_name.clone(),
                            content_type: catalog.content_type.clone(),
                            catalog_id: catalog.id.clone(),
                            catalog_name: if catalog.name.trim().is_empty() {
                                catalog.id.clone()
                            } else {
                                catalog.name.clone()
                            },
                            genre_options: genre
                                .map(|extra| extra.options.clone())
                                .unwrap_or_default(),
                            genre_required: genre.map(|extra| extra.is_required).unwrap_or(false),
                        })
                    })
            })
            .collect()
    }

    /// Re-reads one addon's manifest, matching Nuvio's `refreshAddon`: it picks
    /// up new catalogs, resources and version numbers without touching the
    /// synced row, so it is not a reinstall.
    /// Catalogs that can be browsed without a search term, mirroring Nuvio's
    /// `AddonCatalog.supportsDiscover()`: a required `search` disqualifies a
    /// catalog, `skip` never does, and a required `genre` is fine as long as the
    /// manifest actually lists options to choose from.
    pub fn discover_catalogs(&mut self, addons: &[AddonRow]) -> Vec<DiscoverCatalog> {
        self.ensure_manifests(addons);
        let mut seen = HashSet::new();
        self.manifests
            .iter()
            .flat_map(|installed| {
                installed
                    .manifest
                    .catalogs
                    .iter()
                    .map(move |catalog| (installed, catalog))
            })
            .filter(|(_, catalog)| supports_discover(catalog))
            .filter_map(|(installed, catalog)| {
                let key = format!(
                    "{}:{}:{}",
                    installed.manifest.id, catalog.content_type, catalog.id
                );
                if !seen.insert(key.clone()) {
                    return None;
                }
                let genre = catalog.extra.iter().find(|extra| extra.name == "genre");
                Some(DiscoverCatalog {
                    key,
                    addon_name: installed.display_name.clone(),
                    manifest_url: installed.url.clone(),
                    content_type: catalog.content_type.clone(),
                    catalog_id: catalog.id.clone(),
                    catalog_name: if catalog.name.trim().is_empty() {
                        catalog.id.clone()
                    } else {
                        catalog.name.clone()
                    },
                    genre_options: genre.map(|extra| extra.options.clone()).unwrap_or_default(),
                    genre_required: genre.map(|extra| extra.is_required).unwrap_or(false),
                    supports_pagination: catalog
                        .extra
                        .iter()
                        .any(|extra| extra.name.eq_ignore_ascii_case("skip")),
                })
            })
            .collect()
    }

    pub fn refresh_addon(&mut self, addon: &AddonRow) -> Result<AddonDescriptor> {
        let installed = fetch_manifest(&self.client, addon)?;
        // Replace in place. Manifest position is addon priority, so a refresh
        // must not quietly promote or demote the addon, and the cached
        // signature still holds because the addon *set* has not changed —
        // invalidating it would force a refetch of every other addon.
        match self
            .manifests
            .iter()
            .position(|item| item.url == installed.url)
        {
            Some(index) => self.manifests[index] = installed,
            None => self.manifests.push(installed),
        }
        Ok(self.describe(addon, &[]))
    }

    pub fn load_home(&mut self, addons: &[AddonRow], plan: &HomeLayoutPlan) -> HomePayload {
        let mut errors = self.ensure_manifests(addons);
        // Order before capping so the rows the user put on top are the ones
        // actually fetched — Nuvio's `prioritizeDefinitions` does the same.
        let mut tasks = self.catalog_tasks(false);
        tasks.retain(|task| plan.is_enabled(&task.pref_key));
        tasks.sort_by_key(|task| plan.order_of(&task.pref_key));
        tasks.truncate(HOME_CATALOG_LIMIT);

        let client = &self.client;
        let mut results: Vec<_> = content_pool().install(|| {
            tasks
                .par_iter()
                .enumerate()
                .map(|(index, task)| (index, fetch_catalog(client, task, None, 20)))
                .collect()
        });
        results.sort_by_key(|(index, _)| *index);

        let mut sections = Vec::new();
        for (_, result) in results {
            match result {
                Ok(mut section) => {
                    if plan.hide_unreleased_content {
                        section.items.retain(|item| {
                            !crate::home_layout::is_unreleased(
                                item.released.as_deref(),
                                item.release_info.as_deref(),
                            )
                        });
                    }
                    if section.items.is_empty() {
                        continue;
                    }
                    section.title = match plan.custom_title(&section.pref_key) {
                        Some(title) => title.to_string(),
                        None if plan.show_catalog_type => {
                            format!(
                                "{} - {}",
                                section.title,
                                media_type_label(&section.content_type)
                            )
                        }
                        None => section.title.clone(),
                    };
                    sections.push(section);
                }
                Err(error) => errors.push(error.to_string()),
            }
        }

        // Round-robin the installed catalogs so the first addon does not own
        // every carousel slot, and avoid repeated ids across catalogs.
        let mut hero_items = Vec::new();
        if plan.hero_enabled {
            let hero_sections: Vec<&CatalogSection> = sections
                .iter()
                .filter(|section| plan.is_hero_source(&section.pref_key))
                .collect();
            let mut seen = HashSet::new();
            let max_items = hero_sections
                .iter()
                .map(|section| section.items.len())
                .max()
                .unwrap_or_default();
            'rows: for item_index in 0..max_items {
                for section in &hero_sections {
                    let Some(item) = section.items.get(item_index) else {
                        continue;
                    };
                    let identity = format!("{}:{}", item.content_type, item.id);
                    if seen.insert(identity)
                        && (item.background.is_some()
                            || item.banner.is_some()
                            || item.poster.is_some())
                    {
                        hero_items.push(item.clone());
                        if hero_items.len() == HOME_HERO_ITEM_LIMIT {
                            break 'rows;
                        }
                    }
                }
            }
        }

        // Only advertise rows we can actually render: a catalog that returned
        // nothing would otherwise leave a hole in the ordered list.
        let renderable: HashSet<&str> = sections
            .iter()
            .map(|section| section.pref_key.as_str())
            .collect();
        let rows = plan
            .rows
            .iter()
            .filter(|row| row.is_collection || renderable.contains(row.key.as_str()))
            .cloned()
            .collect();

        let hero = hero_items.first().cloned();
        HomePayload {
            sections,
            rows,
            hero,
            hero_items,
            errors,
        }
    }

    pub fn search(&mut self, addons: &[AddonRow], query: &str) -> HomePayload {
        let mut errors = self.ensure_manifests(addons);
        let mut tasks = self.catalog_tasks(true);
        tasks.truncate(HOME_CATALOG_LIMIT);
        let client = &self.client;
        let mut results: Vec<_> = content_pool().install(|| {
            tasks
                .par_iter()
                .enumerate()
                .map(|(index, task)| {
                    let extra = format!("search={}", encode_component(query));
                    (index, fetch_catalog(client, task, Some(&extra), 20))
                })
                .collect()
        });
        results.sort_by_key(|(index, _)| *index);
        let mut sections = Vec::new();
        for (_, result) in results {
            match result {
                Ok(mut section) if !section.items.is_empty() => {
                    // Addons commonly name every search catalog "Search", so the
                    // media type is what actually distinguishes the rows. Nuvio
                    // titles these "{catalog} • {type}" via discover_catalog_context —
                    // note the bullet, where the home rows use a hyphen.
                    section.title = format!(
                        "{} • {}",
                        section.title,
                        media_type_label(&section.content_type)
                    );
                    sections.push(section);
                }
                Ok(_) => {}
                Err(error) => errors.push(error.to_string()),
            }
        }
        HomePayload {
            sections,
            rows: Vec::new(),
            hero: None,
            hero_items: Vec::new(),
            errors,
        }
    }

    pub fn catalog(
        &mut self,
        addons: &[AddonRow],
        manifest_url: &str,
        content_type: &str,
        catalog_id: &str,
        genre: Option<&str>,
        skip: usize,
    ) -> Result<CatalogSection> {
        self.ensure_manifests(addons);
        let installed = self
            .manifests
            .iter()
            .find(|item| item.url == manifest_url)
            .context("the catalog addon is no longer installed")?;
        let catalog = installed
            .manifest
            .catalogs
            .iter()
            .find(|catalog| catalog.content_type == content_type && catalog.id == catalog_id)
            .context("the addon no longer exposes this catalog")?;
        let task = CatalogTask {
            key: format!("{}:{}:{}", installed.url, content_type, catalog_id),
            pref_key: format!("{}:{}:{}", installed.manifest.id, content_type, catalog_id),
            title: if catalog.name.is_empty() {
                catalog.id.clone()
            } else {
                catalog.name.clone()
            },
            subtitle: installed.display_name.clone(),
            manifest_url: installed.url.clone(),
            content_type: content_type.to_string(),
            catalog_id: catalog_id.to_string(),
            genre: genre
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        };
        let extra = catalog_extras(task.genre.as_deref(), skip);
        fetch_catalog(&self.client, &task, extra.as_deref(), 60)
    }

    fn collection_catalog_task(
        &self,
        addon_id: &str,
        content_type: &str,
        catalog_id: &str,
        genre: Option<&str>,
    ) -> Result<CatalogTask> {
        let installed = self
            .manifests
            .iter()
            .find(|item| item.manifest.id == addon_id)
            .with_context(|| format!("collection addon {addon_id} is not installed"))?;
        let catalog = installed
            .manifest
            .catalogs
            .iter()
            .find(|catalog| catalog.content_type == content_type && catalog.id == catalog_id)
            .context("the collection source catalog is no longer available")?;
        let genre = genre.map(str::trim).filter(|value| !value.is_empty());
        let base_title = if catalog.name.is_empty() {
            catalog.id.clone()
        } else {
            catalog.name.clone()
        };
        Ok(CatalogTask {
            key: format!(
                "collection:{}:{}:{}:{}",
                addon_id,
                content_type,
                catalog_id,
                genre.unwrap_or_default()
            ),
            pref_key: format!("{addon_id}:{content_type}:{catalog_id}"),
            title: genre
                .map(|value| format!("{base_title} · {value}"))
                .unwrap_or(base_title),
            subtitle: installed.display_name.clone(),
            manifest_url: installed.url.clone(),
            content_type: content_type.to_string(),
            catalog_id: catalog_id.to_string(),
            genre: genre.map(str::to_string),
        })
    }

    /// Loads every catalog in a collection folder at once.
    ///
    /// Nuvio gives each folder tab its own coroutine, so a ten-catalog folder
    /// costs one round trip rather than ten. Fetching these in sequence is what
    /// made a large folder sit there — the addons are independent hosts and
    /// nothing here depends on the previous response.
    pub fn collection_folder(
        &mut self,
        addons: &[AddonRow],
        sources: &[crate::collections::CollectionCatalogSource],
    ) -> (Vec<CatalogSection>, Vec<String>) {
        let mut errors = self.ensure_manifests(addons);
        let mut tasks = Vec::new();
        for source in sources {
            match self.collection_catalog_task(
                &source.addon_id,
                &source.content_type,
                &source.catalog_id,
                source.genre.as_deref(),
            ) {
                Ok(task) => tasks.push(task),
                Err(error) => errors.push(error.to_string()),
            }
        }

        let client = &self.client;
        let mut results: Vec<_> = content_pool().install(|| {
            tasks
                .par_iter()
                .enumerate()
                .map(|(index, task)| {
                    let extra = catalog_extras(task.genre.as_deref(), 0);
                    (
                        index,
                        fetch_catalog(client, task, extra.as_deref(), COLLECTION_CATALOG_LIMIT),
                    )
                })
                .collect()
        });
        // The folder's own source order is meaningful, so restore it.
        results.sort_by_key(|(index, _)| *index);

        let mut sections = Vec::new();
        for (_, result) in results {
            match result {
                Ok(section) if !section.items.is_empty() => sections.push(section),
                Ok(_) => {}
                Err(error) => errors.push(error.to_string()),
            }
        }
        (sections, errors)
    }

    pub fn details(
        &mut self,
        addons: &[AddonRow],
        manifest_url: &str,
        content_type: &str,
        id: &str,
    ) -> Result<ContentMeta> {
        self.ensure_manifests(addons);
        let addon_name = self
            .manifests
            .iter()
            .find(|item| item.url == manifest_url)
            .map(|item| item.display_name.clone())
            .unwrap_or_default();
        let url = resource_url(manifest_url, "meta", content_type, id, None)?;
        let payload: Value = get_json(&self.client, &url)?;
        let raw = payload
            .get("meta")
            .or_else(|| payload.pointer("/data/meta"))
            .or_else(|| payload.get("data"))
            .unwrap_or(&payload)
            .clone();
        let mut meta = parse_meta(&raw).context("addon returned invalid metadata")?;
        if meta.id.is_empty() {
            meta.id = id.to_string();
        }
        if meta.content_type.is_empty() {
            meta.content_type = content_type.to_string();
        }
        meta.source_manifest_url = manifest_url.to_string();
        meta.addon_name = addon_name;
        Ok(meta)
    }

    pub fn resolve_meta(
        &mut self,
        addons: &[AddonRow],
        content_type: &str,
        id: &str,
    ) -> Result<ContentMeta> {
        self.ensure_manifests(addons);
        let mut type_candidates = vec![content_type];
        match content_type.to_ascii_lowercase().as_str() {
            "tv" | "show" | "tvshow" | "anime" => type_candidates.push("series"),
            "film" => type_candidates.push("movie"),
            _ => {}
        }
        let compatible = type_candidates
            .iter()
            .flat_map(|candidate| {
                self.manifests
                    .iter()
                    .filter(|item| supports(&item.manifest, "meta", candidate, id))
                    .cloned()
                    .map(|installed| (installed, (*candidate).to_string()))
            })
            .collect::<Vec<_>>();
        if compatible.is_empty() {
            bail!("no installed addon can resolve this progress item");
        }

        let mut failures = Vec::new();
        for (installed, resolved_type) in compatible {
            match self.details(addons, &installed.url, &resolved_type, id) {
                Ok(meta) => return Ok(meta),
                Err(error) => failures.push(format!("{}: {error}", installed.display_name)),
            }
        }
        bail!(
            "installed metadata addons could not resolve this progress item ({})",
            failures.join("; ")
        )
    }

    pub fn streams(
        &mut self,
        addons: &[AddonRow],
        content_type: &str,
        id: &str,
    ) -> Result<Vec<StreamSource>> {
        let _ = self.ensure_manifests(addons);
        let compatible: Vec<_> = self
            .manifests
            .iter()
            .filter(|installed| supports(&installed.manifest, "stream", content_type, id))
            .cloned()
            .collect();
        let client = &self.client;
        let results: Vec<_> = content_pool().install(|| {
            compatible
                .par_iter()
                .map(|installed| {
                    let url = resource_url(&installed.url, "stream", content_type, id, None)?;
                    let payload: Value = get_json(client, &url)?;
                    let values = payload
                        .get("streams")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let mut streams = Vec::new();
                    for value in values {
                        if let Ok(mut stream) = serde_json::from_value::<StreamSource>(value) {
                            if stream.description.trim().is_empty() {
                                stream.description = stream.title.clone();
                            }
                            stream.addon_name = installed.display_name.clone();
                            stream.addon_id = installed.manifest.id.clone();
                            stream.addon_logo = installed
                                .manifest
                                .logo
                                .as_deref()
                                .and_then(|logo| resolve_asset_url(&installed.url, logo));
                            streams.push(stream);
                        }
                    }
                    Ok::<_, anyhow::Error>(streams)
                })
                .collect()
        });
        let mut streams = Vec::new();
        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok(mut value) => streams.append(&mut value),
                Err(error) => errors.push(error.to_string()),
            }
        }
        if streams.is_empty() && !errors.is_empty() {
            bail!(errors.join("; "));
        }
        Ok(streams)
    }

    fn ensure_manifests(&mut self, addons: &[AddonRow]) -> Vec<String> {
        let enabled: Vec<_> = addons
            .iter()
            .filter(|addon| addon.enabled)
            .take(32)
            .cloned()
            .collect();
        let signature = enabled
            .iter()
            .map(|addon| format!("{}:{}", addon.sort_order, addon.url))
            .collect::<Vec<_>>()
            .join("|");
        if signature == self.addon_signature {
            return Vec::new();
        }
        let client = &self.client;
        let results: Vec<_> = content_pool().install(|| {
            enabled
                .par_iter()
                .map(|addon| fetch_manifest(client, addon))
                .collect()
        });
        self.manifests.clear();
        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok(manifest) => self.manifests.push(manifest),
                Err(error) => errors.push(error.to_string()),
            }
        }
        self.addon_signature = signature;
        errors
    }

    fn catalog_tasks(&self, search: bool) -> Vec<CatalogTask> {
        let mut seen = HashSet::new();
        self.manifests
            .iter()
            .flat_map(|installed| {
                installed
                    .manifest
                    .catalogs
                    .iter()
                    .filter_map(|catalog| {
                        let has_search = catalog.extra.iter().any(|extra| extra.name == "search");
                        let unsupported_required = catalog
                            .extra
                            .iter()
                            .any(|extra| extra.is_required && (!search || extra.name != "search"));
                        // Home accepts catalogs with any optional extras (including search),
                        // matching Nuvio's `extra.none { it.isRequired }` rule. Search is
                        // narrower and only targets catalogs that explicitly advertise it.
                        if unsupported_required || (search && !has_search) {
                            return None;
                        }
                        let key =
                            format!("{}:{}:{}", installed.url, catalog.content_type, catalog.id);
                        if !seen.insert(key.clone()) {
                            return None;
                        }
                        Some(CatalogTask {
                            key,
                            pref_key: format!(
                                "{}:{}:{}",
                                installed.manifest.id, catalog.content_type, catalog.id
                            ),
                            title: if catalog.name.is_empty() {
                                catalog.id.clone()
                            } else {
                                catalog.name.clone()
                            },
                            subtitle: installed.display_name.clone(),
                            manifest_url: installed.url.clone(),
                            content_type: catalog.content_type.clone(),
                            catalog_id: catalog.id.clone(),
                            genre: None,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The catalogs the home organizer can reorder. Keyed by manifest id — the
    /// key Nuvio syncs — and deduplicated the same way `buildHomeCatalogDefinitions`
    /// does with `distinctBy(key)`.
    pub fn home_catalog_definitions(&mut self, addons: &[AddonRow]) -> Vec<CatalogDefinition> {
        self.ensure_manifests(addons);
        let mut seen = HashSet::new();
        self.manifests
            .iter()
            .flat_map(|installed| {
                installed
                    .manifest
                    .catalogs
                    .iter()
                    .map(move |catalog| (installed, catalog))
            })
            .filter(|(_, catalog)| !catalog.extra.iter().any(|extra| extra.is_required))
            .filter_map(|(installed, catalog)| {
                let key = format!(
                    "{}:{}:{}",
                    installed.manifest.id, catalog.content_type, catalog.id
                );
                if !seen.insert(key.clone()) {
                    return None;
                }
                Some(CatalogDefinition {
                    key,
                    addon_id: installed.manifest.id.clone(),
                    content_type: catalog.content_type.clone(),
                    catalog_id: catalog.id.clone(),
                    catalog_name: if catalog.name.trim().is_empty() {
                        catalog.id.clone()
                    } else {
                        catalog.name.clone()
                    },
                    addon_name: installed.display_name.clone(),
                })
            })
            .collect()
    }
}

#[derive(Clone)]
struct CatalogTask {
    key: String,
    pref_key: String,
    title: String,
    subtitle: String,
    manifest_url: String,
    content_type: String,
    catalog_id: String,
    genre: Option<String>,
}

/// Mirrors `AddonCatalog.supportsDiscover()`.
fn supports_discover(catalog: &ManifestCatalog) -> bool {
    if catalog
        .extra
        .iter()
        .any(|extra| extra.name == "search" && extra.is_required)
    {
        return false;
    }
    !catalog.extra.iter().any(|extra| match extra.name.as_str() {
        // A required genre is browsable as long as the manifest offers choices.
        "genre" => extra.is_required && extra.options.is_empty(),
        "skip" | "search" => false,
        _ => extra.is_required,
    })
}

fn catalog_extras(genre: Option<&str>, skip: usize) -> Option<String> {
    let mut extras = Vec::new();
    if let Some(genre) = genre.map(str::trim).filter(|value| !value.is_empty()) {
        extras.push(format!("genre={}", encode_component(genre)));
    }
    if skip > 0 {
        extras.push(format!("skip={skip}"));
    }
    (!extras.is_empty()).then(|| extras.join("&"))
}

fn fetch_manifest(client: &Client, addon: &AddonRow) -> Result<InstalledManifest> {
    let url = normalize_manifest_url(&addon.url)?;
    let manifest: Manifest = get_json(client, &url).with_context(|| {
        format!(
            "Could not load addon {}",
            addon.name.as_deref().unwrap_or(&addon.url)
        )
    })?;
    let display_name = addon
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| manifest.name.clone());
    Ok(InstalledManifest {
        url,
        display_name,
        manifest,
    })
}

fn fetch_catalog(
    client: &Client,
    task: &CatalogTask,
    extra: Option<&str>,
    limit: usize,
) -> Result<CatalogSection> {
    let url = resource_url(
        &task.manifest_url,
        "catalog",
        &task.content_type,
        &task.catalog_id,
        extra,
    )?;
    let payload: Value =
        get_json(client, &url).with_context(|| format!("{} catalog failed", task.subtitle))?;
    let values = payload
        .get("metas")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut items = Vec::new();
    for value in values.into_iter().take(limit) {
        if let Some(mut meta) = parse_meta(&value) {
            if meta.content_type.is_empty() {
                meta.content_type = task.content_type.clone();
            }
            meta.source_manifest_url = task.manifest_url.clone();
            meta.addon_name = task.subtitle.clone();
            items.push(meta);
        }
    }
    Ok(CatalogSection {
        key: task.key.clone(),
        pref_key: task.pref_key.clone(),
        title: task.title.clone(),
        subtitle: task.subtitle.clone(),
        manifest_url: task.manifest_url.clone(),
        content_type: task.content_type.clone(),
        catalog_id: task.catalog_id.clone(),
        genre: task.genre.clone(),
        items,
    })
}

fn parse_meta(value: &Value) -> Option<ContentMeta> {
    let id = value_string(value, "id")?;
    let name = value_string(value, "name")?;
    let videos = value
        .get("videos")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_video).collect())
        .unwrap_or_default();
    Some(ContentMeta {
        id,
        content_type: value_string(value, "type").unwrap_or_default(),
        name,
        poster: value_string(value, "poster"),
        background: value_string(value, "background"),
        banner: value_string(value, "banner"),
        logo: value_string(value, "logo"),
        poster_shape: value_string(value, "posterShape"),
        description: value_string(value, "description"),
        release_info: value_string(value, "releaseInfo"),
        released: value_string(value, "released"),
        imdb_rating: value_string(value, "imdbRating"),
        genres: value_string_list(value, "genres"),
        runtime: value_string(value, "runtime"),
        cast: parse_cast(value),
        director: parse_people_names(value, "director", &["director", "directors"]),
        writer: parse_people_names(value, "writer", &["writer", "writers", "screenplay"]),
        status: value_string(value, "status"),
        age_rating: value_string(value, "ageRating"),
        last_air_date: value_string(value, "lastAirDate"),
        country: value_string(value, "country"),
        awards: value_string(value, "awards"),
        language: value_string(value, "language"),
        website: value_string(value, "website"),
        trailers: parse_trailers(value),
        external_ratings: Vec::new(),
        default_video_id: value
            .pointer("/behaviorHints/defaultVideoId")
            .and_then(Value::as_str)
            .map(str::to_string),
        has_scheduled_videos: value
            .pointer("/behaviorHints/hasScheduledVideos")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        videos,
        source_manifest_url: String::new(),
        addon_name: String::new(),
    })
}

fn parse_video(value: &Value) -> Option<Video> {
    Some(Video {
        id: value_string(value, "id")?,
        title: value_string(value, "title")
            .or_else(|| value_string(value, "name"))
            .unwrap_or_default(),
        season: value_i64(value, "season"),
        episode: value_i64(value, "episode"),
        released: value_string(value, "released"),
        thumbnail: value_string(value, "thumbnail"),
        season_poster: value_string(value, "seasonPoster")
            .or_else(|| value_string(value, "season_poster_path")),
        overview: value_string(value, "overview").or_else(|| value_string(value, "description")),
        runtime: value_i64(value, "runtime"),
        available: value
            .get("available")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

fn parse_cast(value: &Value) -> Vec<MetaPerson> {
    let extras = value.pointer("/app_extras/cast");
    let source = extras.or_else(|| value.get("cast"));
    let mut people = match source {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(name) if !name.trim().is_empty() => Some(MetaPerson {
                    name: name.trim().to_string(),
                    ..Default::default()
                }),
                Value::Object(_) => value_string(item, "name").map(|name| MetaPerson {
                    name,
                    role: value_string(item, "character").or_else(|| value_string(item, "role")),
                    photo: value_string(item, "photo")
                        .or_else(|| value_string(item, "profilePath")),
                    tmdb_id: value_i64(item, "tmdbId"),
                }),
                _ => None,
            })
            .collect(),
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| MetaPerson {
                name: name.to_string(),
                ..Default::default()
            })
            .collect(),
        _ => Vec::new(),
    };
    for person in people_links(value, &["cast", "actor", "actors"]) {
        merge_person(&mut people, person);
    }
    people
}

fn people_links(value: &Value, categories: &[&str]) -> Vec<MetaPerson> {
    value
        .get("links")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|link| {
            value_string(link, "category").is_some_and(|category| {
                categories
                    .iter()
                    .any(|candidate| category.eq_ignore_ascii_case(candidate))
            })
        })
        .filter_map(|link| {
            value_string(link, "name").map(|name| MetaPerson {
                name,
                ..Default::default()
            })
        })
        .collect()
}

fn merge_person(people: &mut Vec<MetaPerson>, candidate: MetaPerson) {
    if let Some(existing) = people
        .iter_mut()
        .find(|item| item.name.eq_ignore_ascii_case(&candidate.name))
    {
        existing.role = existing.role.take().or(candidate.role);
        existing.photo = existing.photo.take().or(candidate.photo);
        existing.tmdb_id = existing.tmdb_id.or(candidate.tmdb_id);
    } else {
        people.push(candidate);
    }
}

fn parse_people_names(value: &Value, field: &str, link_categories: &[&str]) -> Vec<String> {
    let mut names = value_string_list(value, field);
    if let Some(extras) = value.pointer(&format!("/app_extras/{field}s")) {
        match extras {
            Value::Array(items) => names.extend(items.iter().filter_map(|item| match item {
                Value::String(name) => Some(name.trim().to_string()),
                Value::Object(_) => value_string(item, "name"),
                _ => None,
            })),
            Value::String(text) => names.extend(text.split(',').map(str::trim).map(str::to_string)),
            _ => {}
        }
    }
    names.extend(
        people_links(value, link_categories)
            .into_iter()
            .map(|person| person.name),
    );
    names.retain(|name| !name.trim().is_empty());
    let mut seen = HashSet::new();
    names.retain(|name| seen.insert(name.to_ascii_lowercase()));
    names
}

fn parse_trailers(value: &Value) -> Vec<MetaTrailer> {
    value
        .get("trailers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let key = value_string(item, "key")
                .or_else(|| value_string(item, "source"))
                .or_else(|| value_string(item, "ytId"))
                .or_else(|| value_string(item, "ytid"))?;
            Some(MetaTrailer {
                id: value_string(item, "id").unwrap_or_else(|| key.clone()),
                key,
                name: value_string(item, "name").unwrap_or_else(|| "Trailer".to_string()),
                site: value_string(item, "site").unwrap_or_else(|| "YouTube".to_string()),
                trailer_type: value_string(item, "type").unwrap_or_else(|| "Trailer".to_string()),
                official: item.get("official").and_then(Value::as_bool),
                published_at: value_string(item, "publishedAt")
                    .or_else(|| value_string(item, "published_at")),
                season_number: value_i64(item, "seasonNumber")
                    .or_else(|| value_i64(item, "season_number")),
            })
        })
        .collect()
}

fn default_true() -> bool {
    true
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(text) => Some(text.clone()).filter(|text| !text.trim().is_empty()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        _ => None,
    }
}

fn value_i64(value: &Value, key: &str) -> Option<i64> {
    value
        .get(key)
        .and_then(|item| item.as_i64().or_else(|| item.as_str()?.parse().ok()))
}

fn value_string_list(value: &Value, key: &str) -> Vec<String> {
    match value.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
                Value::Number(number) => Some(number.to_string()),
                _ => None,
            })
            .collect(),
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn supports(manifest: &Manifest, resource_name: &str, content_type: &str, id: &str) -> bool {
    manifest.resources.iter().any(|resource| match resource {
        Resource::Name(name) => {
            name == resource_name
                && compatible(&manifest.types, &manifest.id_prefixes, content_type, id)
        }
        Resource::Detailed {
            name,
            types,
            id_prefixes,
        } => name == resource_name && compatible(types, id_prefixes, content_type, id),
    })
}

fn compatible(types: &[String], prefixes: &[String], content_type: &str, id: &str) -> bool {
    (types.is_empty() || types.iter().any(|value| value == content_type))
        && (prefixes.is_empty() || prefixes.iter().any(|prefix| id.starts_with(prefix)))
}

fn get_json<T: for<'de> Deserialize<'de>>(client: &Client, url: &str) -> Result<T> {
    validate_addon_url(&Url::parse(url).context("invalid addon resource URL")?)?;
    let mut response = client
        .get(url)
        .send()
        .with_context(|| format!("Request failed: {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("Addon returned HTTP {status}: {url}");
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_ADDON_JSON_BYTES as u64)
    {
        bail!("Addon response exceeded the 8 MiB safety limit: {url}");
    }
    let mut payload = Vec::new();
    response
        .by_ref()
        .take((MAX_ADDON_JSON_BYTES + 1) as u64)
        .read_to_end(&mut payload)
        .with_context(|| format!("Could not read addon response: {url}"))?;
    if payload.len() > MAX_ADDON_JSON_BYTES {
        bail!("Addon response exceeded the 8 MiB safety limit: {url}");
    }
    serde_json::from_slice(&payload).with_context(|| format!("Addon returned invalid JSON: {url}"))
}

fn normalize_manifest_url(input: &str) -> Result<String> {
    let mut url = Url::parse(input.trim()).context("invalid addon URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("addon URL must use http or https");
    }
    validate_addon_url(&url)?;
    if !url.path().trim_end_matches('/').ends_with("manifest.json") {
        let path = format!("{}/manifest.json", url.path().trim_end_matches('/'));
        url.set_path(&path);
    }
    Ok(url.to_string())
}

pub(crate) fn validate_addon_url(url: &Url) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        bail!("addon URL must use http or https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("addon URLs cannot contain embedded credentials");
    }
    let host = url.host_str().context("addon URL is missing a host")?;
    if allow_private_addons() {
        return Ok(());
    }
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        bail!("local/private addon URLs require NUVIO_ALLOW_PRIVATE_ADDONS=1");
    }
    if let Ok(address) = host.parse::<IpAddr>() {
        if is_private_address(address) {
            bail!("local/private addon URLs require NUVIO_ALLOW_PRIVATE_ADDONS=1");
        }
        return Ok(());
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("could not resolve addon host {host}"))?;
    if addresses
        .into_iter()
        .any(|address| is_private_address(address.ip()))
    {
        bail!("addon host resolved to a local/private address");
    }
    Ok(())
}

fn allow_private_addons() -> bool {
    std::env::var("NUVIO_ALLOW_PRIVATE_ADDONS")
        .is_ok_and(|value| matches!(value.trim(), "1" | "true" | "yes"))
}

fn is_private_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_multicast()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 198 && matches!(octets[1], 18 | 19))
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address.is_multicast()
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|address| is_private_address(IpAddr::V4(address)))
        }
    }
}

fn configure_url(manifest_url: &str) -> String {
    let base = manifest_url
        .split('?')
        .next()
        .unwrap_or(manifest_url)
        .trim_end_matches('/');
    format!(
        "{}/configure",
        base.strip_suffix("/manifest.json").unwrap_or(base)
    )
}

fn resolve_asset_url(manifest_url: &str, asset: &str) -> Option<String> {
    Url::parse(asset)
        .ok()
        .map(|url| url.to_string())
        .or_else(|| {
            Url::parse(manifest_url)
                .ok()?
                .join(asset)
                .ok()
                .map(|url| url.to_string())
        })
}

fn resource_url(
    manifest_url: &str,
    resource: &str,
    content_type: &str,
    id: &str,
    extra: Option<&str>,
) -> Result<String> {
    let mut url = Url::parse(manifest_url).context("invalid manifest URL")?;
    let query = url.query().map(str::to_string);
    url.set_query(None);
    let base = url
        .path()
        .trim_end_matches('/')
        .strip_suffix("/manifest.json")
        .unwrap_or(url.path().trim_end_matches('/'));
    let mut path = format!(
        "{base}/{}/{}/{}",
        encode_component(resource),
        encode_component(content_type),
        encode_component(id)
    );
    if let Some(extra) = extra.filter(|value| !value.is_empty()) {
        path.push('/');
        path.push_str(extra);
    }
    path.push_str(".json");
    url.set_path(&path);
    url.set_query(query.as_deref());
    Ok(url.to_string())
}

fn encode_component(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (*byte as char).to_string()
            }
            byte => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_addon_base_and_preserves_query() {
        assert_eq!(
            normalize_manifest_url("https://example.com/addon?token=a").unwrap(),
            "https://example.com/addon/manifest.json?token=a"
        );
    }

    #[test]
    fn builds_stremio_resource_urls() {
        assert_eq!(
            resource_url(
                "https://example.com/a/manifest.json?x=1",
                "catalog",
                "movie",
                "top films",
                Some("search=star%20wars")
            )
            .unwrap(),
            "https://example.com/a/catalog/movie/top%20films/search=star%20wars.json?x=1"
        );
    }

    #[test]
    fn encodes_utf8_components() {
        assert_eq!(encode_component("Amélie: 1"), "Am%C3%A9lie%3A%201");
    }

    #[test]
    fn rejects_private_and_credentialed_addon_urls() {
        assert!(normalize_manifest_url("http://127.0.0.1:7000").is_err());
        assert!(normalize_manifest_url("http://[::1]:7000").is_err());
        assert!(normalize_manifest_url("https://user:pass@8.8.8.8/addon").is_err());
    }

    #[test]
    fn accepts_and_normalizes_public_addon_urls() {
        assert_eq!(
            normalize_manifest_url("https://8.8.8.8/addon").unwrap(),
            "https://8.8.8.8/addon/manifest.json"
        );
    }

    #[test]
    fn home_includes_catalogs_with_optional_search() {
        let service = ContentService {
            client: Client::new(),
            addon_signature: String::new(),
            manifests: vec![InstalledManifest {
                url: "https://example.com/manifest.json".to_string(),
                display_name: "Example".to_string(),
                manifest: Manifest {
                    id: "example".to_string(),
                    name: "Example".to_string(),
                    version: "1.2.3".to_string(),
                    logo: None,
                    types: vec!["movie".to_string()],
                    id_prefixes: Vec::new(),
                    resources: Vec::new(),
                    catalogs: vec![ManifestCatalog {
                        id: "popular".to_string(),
                        content_type: "movie".to_string(),
                        name: "Popular".to_string(),
                        extra: vec![CatalogExtra {
                            name: "search".to_string(),
                            is_required: false,
                            options: Vec::new(),
                        }],
                    }],
                    behavior_hints: ManifestBehaviorHints::default(),
                },
            }],
        };

        assert_eq!(service.catalog_tasks(false).len(), 1);
        assert_eq!(service.catalog_tasks(true).len(), 1);
    }

    fn installed(url: &str, id: &str, version: &str) -> InstalledManifest {
        InstalledManifest {
            url: url.to_string(),
            display_name: id.to_string(),
            manifest: Manifest {
                id: id.to_string(),
                name: id.to_string(),
                version: version.to_string(),
                logo: None,
                types: Vec::new(),
                id_prefixes: Vec::new(),
                resources: Vec::new(),
                catalogs: Vec::new(),
                behavior_hints: ManifestBehaviorHints::default(),
            },
        }
    }

    #[test]
    fn a_refreshed_manifest_keeps_its_priority_slot() {
        // Regression: refresh used to remove-then-append, which silently moved
        // the addon to the bottom. Manifest order is addon priority — Nuvio
        // resolves metadata by taking the first addon that answers.
        let mut service = ContentService {
            client: Client::new(),
            addon_signature: "cached".to_string(),
            manifests: vec![
                installed("https://a.test/manifest.json", "a", "1.0.0"),
                installed("https://b.test/manifest.json", "b", "1.0.0"),
                installed("https://c.test/manifest.json", "c", "1.0.0"),
            ],
        };

        let refreshed = installed("https://b.test/manifest.json", "b", "2.0.0");
        match service
            .manifests
            .iter()
            .position(|item| item.url == refreshed.url)
        {
            Some(index) => service.manifests[index] = refreshed,
            None => service.manifests.push(refreshed),
        }

        let order: Vec<&str> = service
            .manifests
            .iter()
            .map(|item| item.manifest.id.as_str())
            .collect();
        assert_eq!(order, vec!["a", "b", "c"]);
        assert_eq!(service.manifests[1].manifest.version, "2.0.0");
        // The addon set did not change, so other manifests must survive.
        assert_eq!(service.manifests.len(), 3);
        assert_eq!(service.addon_signature, "cached");
    }

    #[test]
    fn descriptors_expose_the_live_manifest_version() {
        let service = ContentService {
            client: Client::new(),
            addon_signature: String::new(),
            manifests: vec![installed("https://a.test/manifest.json", "a", "3.1.4")],
        };
        let reachable = service.describe(
            &AddonRow {
                url: "https://a.test/manifest.json".to_string(),
                name: Some("A".to_string()),
                enabled: true,
                sort_order: 0,
            },
            &[],
        );
        assert_eq!(reachable.version, "3.1.4");

        // An unreachable addon has no manifest, so there is no version to show.
        let missing = service.describe(
            &AddonRow {
                url: "https://gone.test/manifest.json".to_string(),
                name: Some("Gone".to_string()),
                enabled: true,
                sort_order: 1,
            },
            &[],
        );
        assert_eq!(missing.version, "");
    }

    #[test]
    fn catalog_meta_parser_tolerates_common_addon_variations() {
        let meta = parse_meta(&serde_json::json!({
            "id": "tt123",
            "type": "series",
            "name": "Example",
            "imdbRating": 7.5,
            "genres": null,
            "cast": "One, Two",
            "videos": [{ "id": "tt123:1:1", "season": "1", "episode": 1 }]
        }))
        .expect("valid tolerant metadata");

        assert_eq!(meta.imdb_rating.as_deref(), Some("7.5"));
        assert_eq!(
            meta.cast
                .iter()
                .map(|person| person.name.as_str())
                .collect::<Vec<_>>(),
            vec!["One", "Two"]
        );
        assert_eq!(meta.videos[0].season, Some(1));
    }

    #[test]
    fn metadata_parser_merges_stremio_people_links() {
        let meta = parse_meta(&serde_json::json!({
            "id": "tt123",
            "type": "movie",
            "name": "Example",
            "app_extras": { "cast": [{ "name": "Lead Actor", "character": "Lead", "photo": "https://example.com/lead.jpg" }] },
            "links": [
                { "name": "Lead Actor", "category": "actor", "url": "stremio:///search?search=Lead" },
                { "name": "Second Actor", "category": "cast", "url": "stremio:///search?search=Second" },
                { "name": "A Director", "category": "director", "url": "stremio:///search?search=Director" },
                { "name": "A Writer", "category": "screenplay", "url": "stremio:///search?search=Writer" }
            ]
        }))
        .unwrap();

        assert_eq!(meta.cast.len(), 2);
        assert_eq!(meta.cast[0].role.as_deref(), Some("Lead"));
        assert_eq!(meta.director, vec!["A Director"]);
        assert_eq!(meta.writer, vec!["A Writer"]);
    }
}

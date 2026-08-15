use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::{
    auth::{AddonRow, AuthService, NuvioProfile},
    collections::Collection,
    content::ContentService,
    downloads::DownloadManager,
    home_layout::{CatalogDefinition, HomeLayout, HomeLayoutPlan},
    metadata::MetadataConfig,
    player::PlayerService,
    settings::SettingsSnapshot,
};

pub struct AppState {
    pub started_at: Instant,
    pub ping_count: u64,
    pub player: Arc<Mutex<PlayerService>>,
    pub auth: AuthService,
    pub profiles: Vec<NuvioProfile>,
    pub active_profile_index: i32,
    pub addons: Vec<AddonRow>,
    pub content: Arc<Mutex<ContentService>>,
    pub downloads: Arc<Mutex<DownloadManager>>,
    pub metadata_config: MetadataConfig,
    /// Profile settings live in memory just like Nuvio's StateFlow-backed
    /// repositories. They are refreshed on profile changes, not page mounts.
    pub settings_snapshot: Option<SettingsSnapshot>,
    pub settings_blob: Option<Value>,
    /// When the blob was last pulled. Settings changed on another device only
    /// arrive on a pull, so a cached snapshot has to expire.
    pub settings_loaded_at: Option<Instant>,
    /// Cached so `content.home` can order rows without a Supabase round trip on
    /// every render. Refreshed whenever the profile, addons or layout change.
    pub home_layout: HomeLayoutPlan,
    pub session_restore_attempted: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            ping_count: 0,
            player: Arc::new(Mutex::new(PlayerService::default())),
            auth: AuthService::default(),
            profiles: Vec::new(),
            active_profile_index: 1,
            addons: Vec::new(),
            content: Arc::new(Mutex::new(ContentService::default())),
            downloads: Arc::new(Mutex::new(DownloadManager::default())),
            metadata_config: MetadataConfig::default(),
            settings_snapshot: None,
            settings_blob: None,
            settings_loaded_at: None,
            home_layout: HomeLayoutPlan::default(),
            session_restore_attempted: false,
        }
    }
}

impl AppState {
    pub fn restore_saved_account(&mut self) -> anyhow::Result<()> {
        if self.session_restore_attempted {
            return Ok(());
        }
        self.session_restore_attempted = true;
        let restored = self.auth.restore_session();
        let restored = match restored {
            Ok(restored) => restored,
            Err(error) => {
                // A transient network/backend failure must remain retryable in
                // this process and must not be treated as a logged-out account.
                self.session_restore_attempted = false;
                return Err(error);
            }
        };
        if restored && let Err(error) = self.refresh_account_data() {
            self.session_restore_attempted = false;
            return Err(error);
        }
        Ok(())
    }

    pub fn refresh_account_data(&mut self) -> anyhow::Result<()> {
        self.profiles = self.auth.profiles()?;
        if !self
            .profiles
            .iter()
            .any(|profile| profile.profile_index == self.active_profile_index)
        {
            self.active_profile_index = self
                .profiles
                .first()
                .map(|profile| profile.profile_index)
                .unwrap_or(1);
        }
        self.refresh_addons()?;
        if let Err(error) = self.refresh_settings() {
            eprintln!("profile settings could not be loaded: {error:#}");
        }
        Ok(())
    }

    pub fn refresh_addons(&mut self) -> anyhow::Result<()> {
        let effective_profile_id = self.effective_addon_profile_id();
        self.addons = self.auth.addons(effective_profile_id)?;
        self.content.lock().unwrap().invalidate();
        // The organizer is per-profile and keyed by the installed catalogs, so
        // it has to be reloaded on every path that swaps either one.
        self.refresh_home_layout();
        Ok(())
    }

    /// Catalogs this device can render, keyed the way Nuvio syncs them.
    pub fn home_catalog_definitions(&self) -> Vec<CatalogDefinition> {
        self.content
            .lock()
            .map(|mut content| content.home_catalog_definitions(&self.addons))
            .unwrap_or_default()
    }

    pub fn synced_collections(&self) -> Vec<Collection> {
        crate::collections::list(&self.auth, self.active_profile_index).unwrap_or_default()
    }

    /// Pulls the organizer from Supabase. Read-only — never pushes, so opening
    /// the app cannot rewrite what another device saved.
    pub fn load_home_layout(&self) -> anyhow::Result<HomeLayout> {
        crate::home_layout::load(
            &self.auth,
            self.active_profile_index,
            self.home_catalog_definitions(),
            &self.synced_collections(),
        )
    }

    /// Best-effort: a layout that will not load must not block the home page,
    /// so the default plan (everything visible, manifest order) stands in.
    pub fn refresh_home_layout(&mut self) {
        self.home_layout = self
            .load_home_layout()
            .map(|layout| layout.plan())
            .unwrap_or_default();
    }

    pub fn refresh_settings(&mut self) -> anyhow::Result<()> {
        // Never expose the previous profile's values while a new profile is
        // being selected, especially if the network request fails.
        self.settings_snapshot = None;
        self.settings_blob = None;
        self.settings_loaded_at = None;
        self.metadata_config = MetadataConfig::default();
        let (snapshot, blob) = crate::settings::load(&self.auth, self.active_profile_index)?;
        self.metadata_config = crate::settings::metadata_config_from_blob(
            &self.auth,
            self.active_profile_index,
            &blob,
        );
        self.settings_loaded_at = Some(Instant::now());
        self.settings_snapshot = Some(snapshot);
        self.settings_blob = Some(blob);
        Ok(())
    }

    pub fn refresh_metadata(&mut self) {
        if let Some(blob) = self.settings_blob.as_ref() {
            self.metadata_config = crate::settings::metadata_config_from_cached_credentials(
                blob,
                &self.metadata_config,
            );
        } else {
            self.metadata_config = MetadataConfig::default();
        }
    }

    pub fn effective_addon_profile_id(&self) -> i32 {
        self.profiles
            .iter()
            .find(|profile| profile.profile_index == self.active_profile_index)
            .filter(|profile| profile.uses_primary_addons && profile.profile_index != 1)
            .map(|_| 1)
            .unwrap_or(self.active_profile_index)
    }

    pub fn push_addons(&mut self, addons: Vec<AddonRow>) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.can_edit_addons(),
            "This profile inherits addons from the primary profile. Switch to the primary profile to edit them."
        );
        let profile_id = self.effective_addon_profile_id();
        self.auth.push_addons(profile_id, &addons)?;
        self.addons = addons
            .into_iter()
            .enumerate()
            .map(|(index, mut addon)| {
                addon.sort_order = index as i32;
                addon
            })
            .collect();
        self.content.lock().unwrap().invalidate();
        // Installing or disabling an addon changes which catalogs the organizer
        // knows about, so its definitions have to be rebuilt.
        self.refresh_home_layout();
        Ok(())
    }

    pub fn can_edit_addons(&self) -> bool {
        !self.profiles.iter().any(|profile| {
            profile.profile_index == self.active_profile_index
                && profile.profile_index != 1
                && profile.uses_primary_addons
        })
    }

    pub fn create_profile(
        &mut self,
        name: String,
        avatar_color_hex: String,
        avatar_id: Option<String>,
        avatar_url: Option<String>,
        uses_primary_addons: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.profiles.len() < 6, "Nuvio supports up to six profiles");
        let next_index = (1..=6)
            .find(|index| {
                !self
                    .profiles
                    .iter()
                    .any(|profile| profile.profile_index == *index)
            })
            .ok_or_else(|| anyhow::anyhow!("No profile slot is available"))?;
        let user_id = self
            .profiles
            .first()
            .map(|profile| profile.user_id.clone())
            .unwrap_or_default();
        let mut profiles = self.profiles.clone();
        profiles.push(NuvioProfile {
            id: String::new(),
            user_id,
            profile_index: next_index,
            name,
            avatar_color_hex,
            avatar_id,
            avatar_url,
            uses_primary_addons,
            uses_primary_plugins: false,
            pin_enabled: false,
        });
        self.auth.push_profiles(&profiles)?;
        self.profiles = self.auth.profiles()?;
        self.active_profile_index = next_index;
        self.refresh_addons()?;
        self.refresh_settings()?;
        Ok(())
    }
}

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
    settings::{ProviderCredentialStore, SettingsSnapshot},
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
    /// Provider secrets use Nuvio's dedicated credentials RPC and must never
    /// be folded into `settings_blob`. The full raw row set is retained so a
    /// later credential edit can preserve integrations unknown to this client.
    pub provider_credentials: Option<ProviderCredentialStore>,
    /// When the blob was last pulled. Settings changed on another device only
    /// arrive on a pull, so a cached snapshot has to expire.
    pub settings_loaded_at: Option<Instant>,
    /// Cached so `content.home` can order rows without a Supabase round trip on
    /// every render. Refreshed whenever the profile, addons or layout change.
    pub home_layout: HomeLayoutPlan,
    /// Set when the addons or profile changed. Loading the organizer means
    /// fetching every installed addon's manifest, which is far too much to do
    /// before the window can draw, so it waits for something to actually want
    /// it — which is the home page, where those manifests are fetched anyway.
    pub home_layout_stale: bool,
    /// Changes whenever the profile, addon set, or organizer plan changes.
    /// A home request captures this value before doing remote work and may
    /// only publish its result if the value still matches afterwards.
    home_layout_generation: u64,
    /// Milliseconds each startup step took, in order. Bootstrap runs several
    /// backend calls one after another and the window shows nothing until they
    /// finish, so when that is slow the only useful question is which one.
    pub boot_timings: Vec<(&'static str, u128)>,
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
            provider_credentials: None,
            settings_loaded_at: None,
            home_layout: HomeLayoutPlan::default(),
            home_layout_stale: true,
            home_layout_generation: 0,
            boot_timings: Vec::new(),
            session_restore_attempted: false,
        }
    }
}

impl AppState {
    /// Times a startup step and records it under `boot_timings`.
    fn timed<T>(&mut self, label: &'static str, step: impl FnOnce(&mut Self) -> T) -> T {
        let started = Instant::now();
        let outcome = step(self);
        let elapsed = started.elapsed().as_millis();
        self.boot_timings.push((label, elapsed));
        if elapsed >= 1000 {
            eprintln!("startup: {label} took {elapsed}ms");
        }
        outcome
    }

    pub fn restore_saved_account(&mut self) -> anyhow::Result<()> {
        if self.session_restore_attempted {
            return Ok(());
        }
        self.session_restore_attempted = true;
        let restored = self.timed("restoreSession", |state| state.auth.restore_session());
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
        // Account refresh can replace the active profile or addon set. Reject
        // any organizer request that was captured before this refresh, even if
        // one of the following network calls fails part-way through.
        self.invalidate_home_layout();
        self.profiles = self.timed("profiles", |state| state.auth.profiles())?;
        if !self
            .profiles
            .iter()
            .any(|profile| profile.profile_index == self.active_profile_index)
        {
            let profile_index = self
                .profiles
                .first()
                .map(|profile| profile.profile_index)
                .unwrap_or(1);
            self.set_active_profile_index(profile_index);
        }
        self.timed("addons", |state| state.refresh_addons())?;
        if let Err(error) = self.timed("settings", |state| state.refresh_settings()) {
            eprintln!("profile settings could not be loaded: {error:#}");
        }
        Ok(())
    }

    pub fn refresh_addons(&mut self) -> anyhow::Result<()> {
        let effective_profile_id = self.effective_addon_profile_id();
        self.addons = self.auth.addons(effective_profile_id)?;
        self.content.lock().unwrap().invalidate();
        // The organizer is per-profile and keyed by the installed catalogs, so
        // it has to be reloaded on every path that swaps either one — but on
        // the next read, not here, where it would sit in front of the window.
        self.invalidate_home_layout();
        Ok(())
    }

    /// Invalidates both the cached plan and every in-flight load derived from
    /// it. This must be called even when the plan is already stale: a second
    /// addon/profile change still has to reject work captured after the first.
    pub fn invalidate_home_layout(&mut self) {
        self.home_layout_stale = true;
        self.home_layout_generation = self.home_layout_generation.wrapping_add(1);
    }

    pub fn set_active_profile_index(&mut self, profile_index: i32) {
        if self.active_profile_index != profile_index {
            self.active_profile_index = profile_index;
            self.invalidate_home_layout();
        }
    }

    /// Captures everything a read-only organizer load needs. The returned
    /// object owns its auth/addon/profile inputs, so its network work can run
    /// after the caller releases the global `AppState` mutex.
    pub fn pending_home_layout_load(&self) -> Option<HomeLayoutLoadSnapshot> {
        self.home_layout_stale.then(|| HomeLayoutLoadSnapshot {
            auth: self.auth.clone(),
            content: Arc::clone(&self.content),
            addons: self.addons.clone(),
            profile_index: self.active_profile_index,
            generation: self.home_layout_generation,
        })
    }

    /// Publishes a remotely loaded plan only if no profile/addon/layout change
    /// occurred while the global mutex was released.
    pub fn commit_home_layout_load(
        &mut self,
        snapshot: &HomeLayoutLoadSnapshot,
        plan: HomeLayoutPlan,
        elapsed_ms: u128,
    ) -> bool {
        if !self.home_layout_stale
            || self.active_profile_index != snapshot.profile_index
            || self.home_layout_generation != snapshot.generation
        {
            return false;
        }
        self.replace_home_layout(plan);
        self.record_home_layout_timing(elapsed_ms);
        true
    }

    /// Stores an authoritative plan (for example from the organizer screen)
    /// and invalidates any older read that is still in flight.
    pub fn replace_home_layout(&mut self, plan: HomeLayoutPlan) {
        self.home_layout = plan;
        self.home_layout_stale = false;
        self.home_layout_generation = self.home_layout_generation.wrapping_add(1);
    }

    fn record_home_layout_timing(&mut self, elapsed_ms: u128) {
        self.boot_timings.push(("homeLayout", elapsed_ms));
        if elapsed_ms >= 1000 {
            eprintln!("startup: homeLayout took {elapsed_ms}ms");
        }
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
        // Split so a slow layout load names its own cause. The three parts fail
        // for completely different reasons: addon manifests are HTTP against
        // third parties, the other two are Supabase.
        let started = std::time::Instant::now();
        let definitions = self.home_catalog_definitions();
        let manifests_ms = started.elapsed().as_millis();

        let started = std::time::Instant::now();
        let collections = self.synced_collections();
        let collections_ms = started.elapsed().as_millis();

        let started = std::time::Instant::now();
        let layout = crate::home_layout::load(
            &self.auth,
            self.active_profile_index,
            definitions,
            &collections,
        );
        let layout_ms = started.elapsed().as_millis();

        if manifests_ms + collections_ms + layout_ms >= 1000 {
            eprintln!(
                "home layout: manifests {manifests_ms}ms, collections {collections_ms}ms, organizer {layout_ms}ms"
            );
        }
        layout
    }

    pub fn refresh_settings(&mut self) -> anyhow::Result<()> {
        // Never expose the previous profile's values while a new profile is
        // being selected, especially if the network request fails.
        self.settings_snapshot = None;
        self.settings_blob = None;
        self.provider_credentials = None;
        self.settings_loaded_at = None;
        self.metadata_config = MetadataConfig::default();
        let (snapshot, blob) = crate::settings::load(&self.auth, self.active_profile_index)?;
        match crate::settings::load_provider_credentials(&self.auth, self.active_profile_index) {
            Ok(credentials) => {
                self.metadata_config =
                    crate::settings::metadata_config_from_blob(&blob, &credentials);
                self.provider_credentials = Some(credentials);
            }
            Err(error) => {
                // Keep the non-secret settings usable during a transient
                // credentials outage. The Integrations screen retries the
                // credential pull instead of treating this as an empty set.
                eprintln!("provider credentials could not be loaded: {error:#}");
            }
        }
        self.settings_loaded_at = Some(Instant::now());
        self.settings_snapshot = Some(snapshot);
        self.settings_blob = Some(blob);
        Ok(())
    }

    pub fn refresh_metadata(&mut self) {
        if let (Some(blob), Some(credentials)) = (
            self.settings_blob.as_ref(),
            self.provider_credentials.as_ref(),
        ) {
            self.metadata_config = crate::settings::metadata_config_from_blob(blob, credentials);
        } else {
            self.metadata_config = MetadataConfig::default();
        }
    }

    /// Snapshot the credential-backed skip provider options while the account
    /// lock is held, then let the network resolver run without that lock. The
    /// client ID is never part of bootstrap/settings serialization.
    pub fn skip_options(&self) -> crate::skip_segments::SkipOptions {
        crate::skip_segments::SkipOptions {
            anime_skip_enabled: self
                .settings_snapshot
                .as_ref()
                .is_some_and(|settings| settings.anime_skip_enabled),
            anime_skip_client_id: self
                .provider_credentials
                .as_ref()
                .map(ProviderCredentialStore::anime_skip_client_id)
                .unwrap_or_default(),
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
        // knows about, so its definitions have to be rebuilt on the next read.
        self.invalidate_home_layout();
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
        self.set_active_profile_index(next_index);
        self.refresh_addons()?;
        self.refresh_settings()?;
        Ok(())
    }
}

/// Immutable input for a lazy home-layout load. In particular, `load` never
/// needs the global `AppState` mutex; only the narrower content-service lock is
/// used while addon manifests are prepared.
#[derive(Clone)]
pub struct HomeLayoutLoadSnapshot {
    auth: AuthService,
    content: Arc<Mutex<ContentService>>,
    addons: Vec<AddonRow>,
    profile_index: i32,
    generation: u64,
}

impl HomeLayoutLoadSnapshot {
    pub fn load(&self) -> anyhow::Result<HomeLayout> {
        let started = Instant::now();
        let definitions = self
            .content
            .lock()
            .map(|mut content| content.home_catalog_definitions(&self.addons))
            .unwrap_or_default();
        let manifests_ms = started.elapsed().as_millis();

        let started = Instant::now();
        let collections =
            crate::collections::list(&self.auth, self.profile_index).unwrap_or_default();
        let collections_ms = started.elapsed().as_millis();

        let started = Instant::now();
        let layout =
            crate::home_layout::load(&self.auth, self.profile_index, definitions, &collections);
        let layout_ms = started.elapsed().as_millis();

        if manifests_ms + collections_ms + layout_ms >= 1000 {
            eprintln!(
                "home layout: manifests {manifests_ms}ms, collections {collections_ms}ms, organizer {layout_ms}ms"
            );
        }
        layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_home_layout_load_commits_once() {
        let mut state = AppState::default();
        let snapshot = state.pending_home_layout_load().unwrap();
        let mut plan = HomeLayoutPlan::default();
        plan.hero_enabled = false;

        assert!(state.commit_home_layout_load(&snapshot, plan, 7));
        assert!(!state.home_layout_stale);
        assert!(!state.home_layout.hero_enabled);
        assert_eq!(state.boot_timings.last(), Some(&("homeLayout", 7)));
        assert!(!state.commit_home_layout_load(&snapshot, HomeLayoutPlan::default(), 8));
    }

    #[test]
    fn invalidation_rejects_an_older_home_layout_load() {
        let mut state = AppState::default();
        let snapshot = state.pending_home_layout_load().unwrap();
        state.invalidate_home_layout();

        assert!(!state.commit_home_layout_load(&snapshot, HomeLayoutPlan::default(), 1));
        assert!(state.home_layout_stale);
        assert!(state.boot_timings.is_empty());
    }

    #[test]
    fn profile_change_rejects_an_older_home_layout_load() {
        let mut state = AppState::default();
        let snapshot = state.pending_home_layout_load().unwrap();
        state.set_active_profile_index(2);

        assert!(!state.commit_home_layout_load(&snapshot, HomeLayoutPlan::default(), 1));
        assert_eq!(state.active_profile_index, 2);
        assert!(state.home_layout_stale);
    }
}

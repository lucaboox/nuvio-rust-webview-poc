use std::{
    env, fs,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;
use crate::paths::app_data_dir;

const CREDENTIAL_SERVICE: &str = "tv.nuvio.rust-webview-poc";
const CREDENTIAL_USER: &str = "supabase-refresh-token";

#[derive(Clone)]
struct BackendConfig {
    primary_url: String,
    fallback_url: Option<String>,
    anon_key: String,
    self_hosted: bool,
}

impl BackendConfig {
    fn load() -> Option<Self> {
        Self::load_custom().or_else(Self::load_official)
    }

    fn load_official() -> Option<Self> {
        let primary_url = config_value("NUVIO_SUPABASE_URL", option_env!("NUVIO_SUPABASE_URL"))?
            .trim()
            .trim_end_matches('/')
            .to_string();
        let anon_key = config_value(
            "NUVIO_SUPABASE_ANON_KEY",
            option_env!("NUVIO_SUPABASE_ANON_KEY"),
        )?
        .trim()
        .to_string();
        if primary_url.is_empty() || anon_key.is_empty() {
            return None;
        }
        let fallback_url = config_value(
            "NUVIO_SUPABASE_FALLBACK_URL",
            option_env!("NUVIO_SUPABASE_FALLBACK_URL"),
        )
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty() && value != &primary_url);
        Some(Self {
            primary_url,
            fallback_url,
            anon_key,
            self_hosted: false,
        })
    }

    fn load_custom() -> Option<Self> {
        let stored =
            serde_json::from_slice::<StoredBackendConfig>(&fs::read(custom_backend_path()).ok()?)
                .ok()?;
        Self::custom(&stored.url, &stored.publishable_key).ok()
    }

    fn custom(url: &str, publishable_key: &str) -> Result<Self> {
        let primary_url = normalize_backend_url(url)?;
        let anon_key = publishable_key.trim().to_string();
        anyhow::ensure!(!anon_key.is_empty(), "Enter the backend publishable key");
        anyhow::ensure!(
            anon_key.len() <= 16 * 1024 && !anon_key.contains(['\r', '\n']),
            "The backend publishable key is invalid"
        );
        Ok(Self {
            primary_url,
            fallback_url: None,
            anon_key,
            self_hosted: true,
        })
    }

    fn same_backend(&self, other: &Self) -> bool {
        self.primary_url == other.primary_url
            && self.anon_key == other.anon_key
            && self.self_hosted == other.self_hosted
    }

    fn base_urls(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.primary_url.as_str()).chain(self.fallback_url.as_deref())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredBackendConfig {
    url: String,
    publishable_key: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredRefreshSession {
    backend_url: String,
    refresh_token: String,
}

fn config_value(name: &str, compiled: Option<&'static str>) -> Option<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| compiled.map(str::to_string))
}

fn normalize_backend_url(value: &str) -> Result<String> {
    let mut url = Url::parse(value.trim()).context("Enter a valid backend URL")?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https") && url.host().is_some(),
        "The backend URL must use HTTP or HTTPS"
    );
    anyhow::ensure!(
        url.username().is_empty() && url.password().is_none(),
        "The backend URL cannot contain credentials"
    );
    anyhow::ensure!(
        url.query().is_none() && url.fragment().is_none(),
        "The backend URL cannot contain a query or fragment"
    );
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&path);
    Ok(url.to_string().trim_end_matches('/').to_string())
}


fn custom_backend_path() -> PathBuf {
    app_data_dir().join("backend.json")
}

fn save_custom_backend(config: &BackendConfig) -> Result<()> {
    let path = custom_backend_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Could not create Nuvio's settings directory")?;
    }
    let stored = StoredBackendConfig {
        url: config.primary_url.clone(),
        publishable_key: config.anon_key.clone(),
    };
    fs::write(path, serde_json::to_vec_pretty(&stored)?)
        .context("Could not save the self-hosted backend configuration")
}

fn clear_custom_backend() -> Result<()> {
    let path = custom_backend_path();
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("Could not clear the self-hosted backend configuration"),
    }
}

#[derive(Clone, Debug)]
struct Session {
    tokens: Arc<SessionTokens>,
    user: AuthUser,
}

#[derive(Debug)]
struct SessionTokens {
    access_token: RwLock<String>,
    /// Serializes refreshes across every clone of an AuthService. Without this,
    /// several requests that observe the same expired JWT can each rotate the
    /// single-use Supabase refresh token and invalidate one another.
    refresh_gate: Mutex<()>,
    /// Stale AuthService clones can outlive sign-out or a backend switch (for
    /// example, a player progress worker). Marking their shared session inactive
    /// prevents them from refreshing or continuing to write after that point.
    active: AtomicBool,
}

impl Session {
    fn new(access_token: String, user: AuthUser) -> Self {
        Self {
            tokens: Arc::new(SessionTokens {
                access_token: RwLock::new(access_token),
                refresh_gate: Mutex::new(()),
                active: AtomicBool::new(true),
            }),
            user,
        }
    }

    fn access_token(&self) -> Result<String> {
        if !self.tokens.active.load(Ordering::Acquire) {
            bail!("The Nuvio session is no longer active. Sign in again.");
        }
        Ok(self
            .tokens
            .access_token
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }

    fn is_active(&self) -> bool {
        self.tokens.active.load(Ordering::Acquire)
    }

    fn is_authenticated(&self) -> bool {
        self.is_active()
            && (self.user.is_anonymous
                || !self
                    .tokens
                    .access_token
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_empty())
    }

    fn deactivate(&self) {
        self.tokens.active.store(false, Ordering::Release);
        self.tokens
            .access_token
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    fn deactivate_and_wait_for_refresh(&self) {
        self.deactivate();
        // A refresh that was already in flight checks `active` before
        // publishing its token. Waiting for its gate also guarantees callers
        // can safely clear or replace the stored refresh credential afterward.
        drop(
            self.tokens
                .refresh_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
    pub is_anonymous: bool,
}

#[derive(Debug, Deserialize)]
struct SupabaseUser {
    id: String,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    user: Option<SupabaseUser>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSnapshot {
    pub status: &'static str,
    pub backend_configured: bool,
    pub official_backend_configured: bool,
    pub self_hosted: bool,
    pub backend_url: Option<String>,
    pub custom_key_saved: bool,
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub is_anonymous: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NuvioProfile {
    #[serde(default)]
    pub id: String,
    #[serde(default, alias = "user_id")]
    pub user_id: String,
    #[serde(default = "default_profile_index", alias = "profile_index")]
    pub profile_index: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_avatar_color", alias = "avatar_color_hex")]
    pub avatar_color_hex: String,
    #[serde(default, alias = "avatar_id")]
    pub avatar_id: Option<String>,
    #[serde(default, alias = "avatar_url")]
    pub avatar_url: Option<String>,
    #[serde(default, alias = "uses_primary_addons")]
    pub uses_primary_addons: bool,
    #[serde(default, alias = "uses_primary_plugins")]
    pub uses_primary_plugins: bool,
    #[serde(default, alias = "pin_enabled")]
    pub pin_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonRow {
    pub url: String,
    pub name: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, alias = "sort_order")]
    pub sort_order: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarCatalogItem {
    pub id: String,
    #[serde(default, alias = "display_name")]
    pub display_name: String,
    #[serde(default, alias = "storage_path")]
    pub storage_path: String,
    #[serde(default)]
    pub category: String,
    #[serde(default, alias = "sort_order")]
    pub sort_order: i32,
    #[serde(default = "default_enabled", alias = "is_active")]
    pub is_active: bool,
    #[serde(default, alias = "bg_color")]
    pub bg_color: Option<String>,
    #[serde(default)]
    pub image_url: String,
}

fn default_profile_index() -> i32 {
    1
}

fn default_avatar_color() -> String {
    "#1E88E5".to_string()
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone)]
pub struct AuthService {
    client: Client,
    config: Option<BackendConfig>,
    session: Option<Session>,
    sync_client_id: String,
    /// Index of the endpoint that answered most recently. Shared across clones
    /// so every one of them benefits from what any of them learned.
    preferred_base: Arc<AtomicUsize>,
}

impl Default for AuthService {
    fn default() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                // A backend that is refusing connections or blackholing them
                // should not cost the whole request budget before the fallback
                // is tried. Startup makes several of these calls in sequence,
                // so 30s of dead primary is 30s of a blank window.
                .connect_timeout(Duration::from_secs(5))
                .user_agent("NuvioRustPoc/0.1.0")
                .build()
                .expect("valid HTTP client"),
            config: BackendConfig::load(),
            session: None,
            sync_client_id: format!("nuvio-rust-{}", Uuid::new_v4().simple()),
            preferred_base: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl AuthService {
    pub fn snapshot(&self) -> AuthSnapshot {
        let self_hosted = self
            .config
            .as_ref()
            .is_some_and(|config| config.self_hosted);
        let backend_url = self
            .config
            .as_ref()
            .filter(|config| config.self_hosted)
            .map(|config| config.primary_url.clone());
        let official_backend_configured = BackendConfig::load_official().is_some();
        match &self.session {
            Some(session) if session.is_authenticated() => AuthSnapshot {
                status: "authenticated",
                backend_configured: self.config.is_some(),
                official_backend_configured,
                self_hosted,
                backend_url,
                custom_key_saved: self_hosted,
                user_id: Some(session.user.id.clone()),
                email: session.user.email.clone(),
                is_anonymous: session.user.is_anonymous,
            },
            _ => AuthSnapshot {
                status: "unauthenticated",
                backend_configured: self.config.is_some(),
                official_backend_configured,
                self_hosted,
                backend_url,
                custom_key_saved: self_hosted,
                user_id: None,
                email: None,
                is_anonymous: false,
            },
        }
    }

    pub fn configure_backend(
        &mut self,
        self_hosted: bool,
        url: Option<&str>,
        publishable_key: Option<&str>,
    ) -> Result<AuthSnapshot> {
        let next = if self_hosted {
            let normalized_url = normalize_backend_url(url.unwrap_or_default())?;
            let key = publishable_key
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    self.config
                        .as_ref()
                        .filter(|config| config.self_hosted && config.primary_url == normalized_url)
                        .map(|config| config.anon_key.clone())
                })
                .context("Enter the backend publishable key")?;
            BackendConfig::custom(&normalized_url, &key)?
        } else {
            BackendConfig::load_official()
                .context("This build has no official Nuvio backend configuration")?
        };

        let changed = self
            .config
            .as_ref()
            .is_none_or(|current| !current.same_backend(&next));
        if self_hosted {
            save_custom_backend(&next)?;
        } else {
            clear_custom_backend()?;
        }
        if changed {
            if let Some(session) = self.session.take() {
                session.deactivate_and_wait_for_refresh();
            }
            clear_refresh_token();
            // The remembered endpoint belonged to the old configuration.
            self.preferred_base.store(0, Ordering::Relaxed);
        }
        self.config = Some(next);
        Ok(self.snapshot())
    }

    pub fn continue_anonymously(&mut self) -> AuthSnapshot {
        if let Some(session) = self.session.take() {
            session.deactivate_and_wait_for_refresh();
        }
        self.session = Some(Session::new(
            String::new(),
            AuthUser {
                id: Uuid::new_v4().to_string(),
                email: None,
                is_anonymous: true,
            },
        ));
        self.snapshot()
    }

    pub fn sign_in(&mut self, email: &str, password: &str) -> Result<AuthSnapshot> {
        self.authenticate("/auth/v1/token?grant_type=password", email, password, false)?;
        Ok(self.snapshot())
    }

    pub fn sign_up(&mut self, email: &str, password: &str) -> Result<(AuthSnapshot, bool)> {
        let has_session = self.authenticate("/auth/v1/signup", email, password, true)?;
        Ok((self.snapshot(), !has_session))
    }

    pub fn restore_session(&mut self) -> Result<bool> {
        let config = self
            .config
            .clone()
            .context("This build has no Nuvio backend configuration")?;
        let Some(refresh_token) = load_refresh_token(&config)? else {
            return Ok(false);
        };
        let response = self.send_with_fallback(&config, |base_url| {
            self.client
                .post(format!("{base_url}/auth/v1/token?grant_type=refresh_token"))
                .header("apikey", &config.anon_key)
                .json(&json!({ "refresh_token": refresh_token }))
                .send()
        });
        let response = response?;
        let status = response.status();
        let payload = match decode_success::<AuthTokenResponse>(response) {
            Ok(payload) => payload,
            Err(_) if matches!(status.as_u16(), 400 | 401 | 403) => {
                clear_refresh_token();
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        if self.install_session(payload)? {
            return Ok(true);
        }
        clear_refresh_token();
        Ok(false)
    }

    fn authenticate(
        &mut self,
        path: &str,
        email: &str,
        password: &str,
        allow_no_session: bool,
    ) -> Result<bool> {
        let config = self
            .config
            .clone()
            .context("This build has no Nuvio backend configuration")?;
        let body = json!({ "email": email, "password": password });
        let response = self.send_with_fallback(&config, |base_url| {
            self.client
                .post(format!("{base_url}{path}"))
                .header("apikey", &config.anon_key)
                .json(&body)
                .send()
        })?;
        let payload = decode_success::<AuthTokenResponse>(response)?;
        self.install_session(payload).and_then(|installed| {
            if installed || allow_no_session {
                Ok(installed)
            } else {
                bail!("The server did not return a usable login session")
            }
        })
    }

    fn install_session(&mut self, payload: AuthTokenResponse) -> Result<bool> {
        match (payload.access_token, payload.refresh_token, payload.user) {
            (Some(access_token), Some(refresh_token), Some(user)) => {
                let config = self
                    .config
                    .as_ref()
                    .context("This build has no Nuvio backend configuration")?;
                // Stop every clone of the previous session before rotating the
                // process-wide saved credential. Otherwise an in-flight clone
                // can overwrite the new refresh token after login completes.
                if let Some(session) = self.session.take() {
                    session.deactivate_and_wait_for_refresh();
                }
                // Persist first so a Credential Manager error never publishes
                // a session that cannot survive an application restart.
                save_refresh_token(config, &refresh_token)?;
                self.session = Some(Session::new(
                    access_token,
                    AuthUser {
                        id: user.id,
                        email: user.email,
                        is_anonymous: false,
                    },
                ));
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn sign_out(&mut self) {
        if let Some(session) = self.session.take() {
            let access_token = session.access_token().unwrap_or_default();
            session.deactivate();
            if let Some(config) = &self.config
                && !session.user.is_anonymous
                && !access_token.is_empty()
            {
                let _ = self
                    .client
                    .post(format!("{}/auth/v1/logout", config.primary_url))
                    .header("apikey", &config.anon_key)
                    .bearer_auth(access_token)
                    .send();
            }
            // If a clone was already refreshing, let it finish before clearing
            // the shared credential. Its post-request active check prevents it
            // from publishing another access token, and this wait makes the
            // credential deletion the final operation.
            session.deactivate_and_wait_for_refresh();
        }
        clear_refresh_token();
    }

    pub fn profiles(&self) -> Result<Vec<NuvioProfile>> {
        let session = self
            .session
            .as_ref()
            .context("Sign in before loading profiles")?;
        if session.user.is_anonymous {
            return Ok(vec![NuvioProfile {
                id: String::new(),
                user_id: session.user.id.clone(),
                profile_index: 1,
                name: "Guest".to_string(),
                avatar_color_hex: "#397A63".to_string(),
                avatar_id: None,
                avatar_url: None,
                uses_primary_addons: false,
                uses_primary_plugins: false,
                pin_enabled: false,
            }]);
        }
        self.authorized_json(
            "/rest/v1/rpc/sync_pull_profiles",
            |client, url, config, token| {
                client
                    .post(url)
                    .header("apikey", &config.anon_key)
                    .bearer_auth(token)
                    .json(&json!({}))
                    .send()
            },
        )
    }

    pub fn avatar_catalog(&self) -> Result<Vec<AvatarCatalogItem>> {
        let session = self
            .session
            .as_ref()
            .context("Sign in before loading avatars")?;
        if session.user.is_anonymous {
            return Ok(Vec::new());
        }
        let config = self
            .config
            .as_ref()
            .context("This build has no Nuvio backend configuration")?;
        let value = self.rpc_value("get_avatar_catalog", &json!({}))?;
        let mut items: Vec<AvatarCatalogItem> = serde_json::from_value(value)?;
        let base = config.primary_url.trim_end_matches('/');
        for item in &mut items {
            if !item.storage_path.trim().is_empty() {
                item.image_url = format!(
                    "{base}/storage/v1/object/public/avatars/{}",
                    item.storage_path.trim_start_matches('/')
                );
            }
        }
        items.retain(|item| item.is_active);
        items.sort_by(|left, right| {
            left.category
                .cmp(&right.category)
                .then(left.sort_order.cmp(&right.sort_order))
        });
        Ok(items)
    }

    pub fn push_profiles(&self, profiles: &[NuvioProfile]) -> Result<()> {
        let payload: Vec<_> = profiles
            .iter()
            .map(|profile| {
                json!({
                    "profile_index": profile.profile_index,
                    "name": profile.name,
                    "avatar_color_hex": profile.avatar_color_hex,
                    "uses_primary_addons": profile.uses_primary_addons,
                    "uses_primary_plugins": profile.uses_primary_plugins,
                    "avatar_id": profile.avatar_id,
                    "avatar_url": profile.avatar_url,
                })
            })
            .collect();
        self.rpc_unit(
            "sync_push_profiles",
            &json!({
                "p_client_max_profiles": 6,
                "p_profiles": payload,
                "p_origin_client_id": self.sync_client_id,
            }),
        )
    }

    pub fn addons(&self, profile_id: i32) -> Result<Vec<AddonRow>> {
        let session = self
            .session
            .as_ref()
            .context("Sign in before loading addons")?;
        if session.user.is_anonymous {
            return Ok(Vec::new());
        }
        self.authorized_json("/rest/v1/addons", |client, url, config, token| {
            client
                .get(url)
                .header("apikey", &config.anon_key)
                .bearer_auth(token)
                .query(&[
                    ("profile_id", format!("eq.{profile_id}")),
                    ("select", "url,name,enabled,sort_order".to_string()),
                    ("order", "sort_order.asc".to_string()),
                ])
                .send()
        })
    }

    pub fn rpc_value(&self, function: &str, params: &Value) -> Result<Value> {
        let response = self.authorized_rpc(function, params)?;
        decode_success(response)
    }

    /// Use for mutation RPCs whose successful PostgREST response may have an
    /// empty body. Decoding those as JSON produces a false client-side error
    /// even though the server already committed the write.
    pub fn rpc_unit(&self, function: &str, params: &Value) -> Result<()> {
        let response = self.authorized_rpc(function, params)?;
        ensure_success(response)
    }

    pub fn push_addons(&self, profile_id: i32, addons: &[AddonRow]) -> Result<()> {
        // `sort_order` is the addon's priority, not just display order: Nuvio
        // resolves metadata by walking enabled addons in this order and taking
        // the first one that answers. It is rewritten as a dense 0-based index
        // on every push, and deduplicated by URL, exactly as Nuvio does — a
        // duplicate row would otherwise shift every addon below it.
        let mut seen = std::collections::HashSet::new();
        let items: Vec<_> = addons
            .iter()
            .filter(|addon| seen.insert(addon.url.clone()))
            .enumerate()
            .map(|(index, addon)| {
                json!({
                    "url": addon.url,
                    "name": addon.name.clone().unwrap_or_default(),
                    "enabled": addon.enabled,
                    "sort_order": index,
                })
            })
            .collect();
        let response = self.authorized_rpc(
            "sync_push_addons",
            &json!({
                "p_profile_id": profile_id,
                "p_addons": items,
                "p_origin_client_id": self.sync_client_id,
            }),
        )?;
        ensure_success(response)
    }

    pub fn sync_client_id(&self) -> &str {
        &self.sync_client_id
    }

    fn refresh_after_unauthorized(
        &self,
        session: &Session,
        rejected_access_token: &str,
    ) -> Result<String> {
        if session.user.is_anonymous {
            bail!("Anonymous sessions cannot be refreshed");
        }
        let _refresh_guard = session
            .tokens
            .refresh_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !session.tokens.active.load(Ordering::Acquire) {
            bail!("The Nuvio session is no longer active. Sign in again.");
        }

        // Another request may have completed the refresh while this one was
        // waiting for the gate. Reuse its token instead of rotating again.
        let current_access_token = session.access_token()?;
        if current_access_token != rejected_access_token {
            if current_access_token.is_empty() {
                bail!("The Nuvio session has no access token. Sign in again.");
            }
            return Ok(current_access_token);
        }

        let config = self
            .config
            .as_ref()
            .context("This build has no Nuvio backend configuration")?;
        let Some(refresh_token) = load_refresh_token(config)? else {
            session.deactivate();
            bail!("The saved Nuvio session has expired. Sign in again.");
        };
        let response = self.send_with_fallback(config, |base_url| {
            self.client
                .post(format!("{base_url}/auth/v1/token?grant_type=refresh_token"))
                .header("apikey", &config.anon_key)
                .json(&json!({ "refresh_token": refresh_token }))
                .send()
        })?;
        let status = response.status();
        let payload = match decode_success::<AuthTokenResponse>(response) {
            Ok(payload) => payload,
            Err(error) if matches!(status.as_u16(), 400 | 401 | 403) => {
                session.deactivate();
                clear_refresh_token();
                return Err(error).context("The saved Nuvio session has expired");
            }
            Err(error) => return Err(error).context("Could not refresh the Nuvio session"),
        };
        let access_token = payload
            .access_token
            .filter(|token| !token.trim().is_empty())
            .context("The backend did not return a refreshed access token")?;
        let refresh_token = payload
            .refresh_token
            .filter(|token| !token.trim().is_empty())
            .context("The backend did not return a rotated refresh token")?;

        // sign_out/configure_backend marks the shared state inactive before it
        // waits for this gate. Never publish credentials after that point.
        if !session.tokens.active.load(Ordering::Acquire) {
            bail!("The Nuvio session is no longer active. Sign in again.");
        }
        save_refresh_token(config, &refresh_token)?;
        let mut shared_access_token = session
            .tokens
            .access_token
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !session.tokens.active.load(Ordering::Acquire) {
            bail!("The Nuvio session is no longer active. Sign in again.");
        }
        *shared_access_token = access_token.clone();
        Ok(access_token)
    }

    fn authorized_rpc(&self, function: &str, params: &Value) -> Result<Response> {
        let config = self
            .config
            .as_ref()
            .context("This build has no Nuvio backend configuration")?;
        let session = self
            .session
            .as_ref()
            .context("Sign in before changing synced Nuvio data")?;
        let send = |access_token: &str| {
            self.send_with_fallback(config, |base_url| {
                self.client
                    .post(format!("{base_url}/rest/v1/rpc/{function}"))
                    .header("apikey", &config.anon_key)
                    .bearer_auth(access_token)
                    .json(params)
                    .send()
            })
        };
        let access_token = session.access_token()?;
        let response = send(&access_token)?;
        if !should_refresh_authorized_request(response.status().as_u16(), session.user.is_anonymous)
        {
            return Ok(response);
        }
        drop(response);
        let refreshed_access_token = self.refresh_after_unauthorized(session, &access_token)?;
        send(&refreshed_access_token)
    }

    fn authorized_json<T, F>(&self, path: &str, request: F) -> Result<T>
    where
        T: DeserializeOwned,
        F: Fn(&Client, String, &BackendConfig, &str) -> Result<Response, reqwest::Error>,
    {
        let config = self
            .config
            .as_ref()
            .context("This build has no Nuvio backend configuration")?;
        let session = self
            .session
            .as_ref()
            .context("Sign in before accessing Nuvio data")?;
        let send = |access_token: &str| {
            self.send_with_fallback(config, |base_url| {
                request(
                    &self.client,
                    format!("{base_url}{path}"),
                    config,
                    access_token,
                )
            })
        };
        let access_token = session.access_token()?;
        let mut response = send(&access_token)?;
        if should_refresh_authorized_request(response.status().as_u16(), session.user.is_anonymous)
        {
            drop(response);
            let refreshed_access_token = self.refresh_after_unauthorized(session, &access_token)?;
            response = send(&refreshed_access_token)?;
        }
        decode_success(response)
    }

    /// Sends to the endpoint that answered last, falling back to the others.
    ///
    /// Startup makes five of these calls in sequence. Beginning each one at the
    /// primary means a primary that is down is paid for five times over, which
    /// is minutes of a window that shows nothing. Remembering which endpoint
    /// answered costs one failure instead, and the order resets whenever the
    /// backend is reconfigured.
    fn send_with_fallback<F>(&self, config: &BackendConfig, request: F) -> Result<Response>
    where
        F: Fn(&str) -> Result<Response, reqwest::Error>,
    {
        let bases: Vec<&str> = config.base_urls().collect();
        if bases.is_empty() {
            return Err(anyhow::anyhow!("No backend endpoint is configured"));
        }
        let start = self.preferred_base.load(Ordering::Relaxed) % bases.len();
        let mut last_error = None;
        for offset in 0..bases.len() {
            let index = (start + offset) % bases.len();
            match request(bases[index]) {
                Ok(response) if !is_origin_failure(response.status().as_u16()) => {
                    self.preferred_base.store(index, Ordering::Relaxed);
                    return Ok(response);
                }
                Ok(response) => {
                    last_error = Some(anyhow::anyhow!(
                        "Backend returned HTTP {}",
                        response.status()
                    ))
                }
                Err(error) => last_error = Some(error.into()),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("No backend endpoint is available")))
    }
}

fn credential_entry() -> Result<keyring::Entry> {
    keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)
        .context("Windows Credential Manager is unavailable")
}

fn load_refresh_token(config: &BackendConfig) -> Result<Option<String>> {
    match credential_entry()?.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(refresh_token_for_backend(&value, config)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error).context("could not read the saved Nuvio session"),
    }
}

fn refresh_token_for_backend(value: &str, config: &BackendConfig) -> Option<String> {
    if let Ok(stored) = serde_json::from_str::<StoredRefreshSession>(value) {
        return (stored.backend_url == config.primary_url)
            .then_some(stored.refresh_token)
            .filter(|token| !token.trim().is_empty());
    }
    // Existing releases stored the official refresh token directly. Never
    // send that unscoped legacy value to a custom backend.
    (!config.self_hosted).then(|| value.to_string())
}

fn save_refresh_token(config: &BackendConfig, refresh_token: &str) -> Result<()> {
    let stored = serde_json::to_string(&StoredRefreshSession {
        backend_url: config.primary_url.clone(),
        refresh_token: refresh_token.to_string(),
    })?;
    credential_entry()?
        .set_password(&stored)
        .context("could not save the Nuvio session in Windows Credential Manager")
}

fn clear_refresh_token() {
    if let Ok(entry) = credential_entry() {
        let _ = entry.delete_credential();
    }
}

fn is_origin_failure(status: u16) -> bool {
    matches!(status, 502..=504 | 520..=526)
}

fn should_refresh_authorized_request(status: u16, is_anonymous: bool) -> bool {
    status == 401 && !is_anonymous
}

fn decode_success<T: DeserializeOwned>(response: Response) -> Result<T> {
    let status = response.status();
    let payload = response.text().context("failed to read backend response")?;
    if !status.is_success() {
        let message = serde_json::from_str::<Value>(&payload)
            .ok()
            .and_then(|value| {
                ["error_description", "msg", "message", "error"]
                    .iter()
                    .find_map(|key| value.get(key).and_then(Value::as_str))
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("Nuvio backend returned HTTP {status}"));
        bail!(message);
    }
    serde_json::from_str(&payload).context("backend returned an unexpected response shape")
}

fn ensure_success(response: Response) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let payload = response.text().unwrap_or_default();
    let message = serde_json::from_str::<Value>(&payload)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("Nuvio backend returned HTTP {status}"));
    bail!(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymous_mode_never_requires_backend_configuration() {
        let mut auth = AuthService::default();
        let snapshot = auth.continue_anonymously();

        assert_eq!(snapshot.status, "authenticated");
        assert!(snapshot.is_anonymous);
        assert_eq!(auth.profiles().unwrap()[0].name, "Guest");
    }

    #[test]
    fn self_hosted_urls_are_normalized_without_changing_their_base_path() {
        assert_eq!(
            normalize_backend_url(" https://nuvio.example.com/supabase/ ").unwrap(),
            "https://nuvio.example.com/supabase"
        );
        assert_eq!(
            normalize_backend_url("http://127.0.0.1:8000/").unwrap(),
            "http://127.0.0.1:8000"
        );
    }

    #[test]
    fn self_hosted_urls_reject_unsafe_or_ambiguous_values() {
        assert!(normalize_backend_url("file:///tmp/nuvio").is_err());
        assert!(normalize_backend_url("https://user:pass@example.com").is_err());
        assert!(normalize_backend_url("https://example.com?tenant=one").is_err());
    }

    #[test]
    fn saved_sessions_are_scoped_to_the_selected_backend() {
        let first = BackendConfig::custom("https://one.example.com", "key-one").unwrap();
        let second = BackendConfig::custom("https://two.example.com", "key-two").unwrap();
        let stored = serde_json::to_string(&StoredRefreshSession {
            backend_url: first.primary_url.clone(),
            refresh_token: "refresh-one".into(),
        })
        .unwrap();

        assert_eq!(
            refresh_token_for_backend(&stored, &first).as_deref(),
            Some("refresh-one")
        );
        assert!(refresh_token_for_backend(&stored, &second).is_none());
        assert!(refresh_token_for_backend("legacy-token", &first).is_none());
    }

    #[test]
    fn cloned_sessions_share_rotated_access_tokens() {
        let session = Session::new(
            "access-one".into(),
            AuthUser {
                id: "user-one".into(),
                email: None,
                is_anonymous: false,
            },
        );
        let clone = session.clone();

        *session
            .tokens
            .access_token
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = "access-two".into();

        assert_eq!(clone.access_token().unwrap(), "access-two");
    }

    #[test]
    fn deactivating_a_session_invalidates_every_clone() {
        let session = Session::new(
            "access-one".into(),
            AuthUser {
                id: "user-one".into(),
                email: None,
                is_anonymous: false,
            },
        );
        let clone = session.clone();

        session.deactivate_and_wait_for_refresh();

        assert!(clone.access_token().is_err());
        assert!(!clone.is_authenticated());
    }

    #[test]
    fn only_authenticated_http_401_responses_trigger_refresh() {
        assert!(should_refresh_authorized_request(401, false));
        assert!(!should_refresh_authorized_request(400, false));
        assert!(!should_refresh_authorized_request(403, false));
        assert!(!should_refresh_authorized_request(500, false));
        assert!(!should_refresh_authorized_request(401, true));
    }
}

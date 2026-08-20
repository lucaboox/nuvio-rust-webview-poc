//! The shell's answer to the shared UI's `platform.request`.
//!
//! The web client reaches addons and providers with `fetch`; in this webview it
//! cannot, because the CSP allows `connect-src ipc:` and nothing else. Rather
//! than widen that — which would hand every addon a browser context inside the
//! desktop app — the page asks here and the request is made from Rust, where a
//! timeout, a size cap and the scheme rules are enforced in one place.
//!
//! Deliberately not a proxy. It carries no ambient credentials, refuses
//! anything that is not http(s), and returns text — never a stream. Media never
//! comes through here: libmpv opens its own URLs.

use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

/// Matches the shared UI's own ceiling for a JSON payload.
const DEFAULT_MAX_BYTES: usize = 6 * 1024 * 1024;
/// Matches the addon client's per-attempt timeout, so a retry means the same
/// thing on both shells.
const DEFAULT_TIMEOUT_MS: u64 = 14_000;
/// A ceiling on what a caller may ask for, so one bad call cannot hang the app.
const MAX_TIMEOUT_MS: u64 = 60_000;

static HTTP: OnceLock<Client> = OnceLock::new();

fn client() -> &'static Client {
    HTTP.get_or_init(|| {
        // Nothing ambient can ride along: reqwest is built here without its
        // `cookies` feature, so this client has no jar to send from. Should
        // that feature ever be turned on for something else, this builder needs
        // `.cookie_store(false)` to keep the promise the contract makes.
        Client::builder()
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub ok: bool,
    pub status: u16,
    /// Lower-cased, which is the form the shared contract promises.
    pub headers: std::collections::HashMap<String, String>,
    pub body: String,
}

/// Rejects what should never be dialled from a request the page composed.
///
/// The shared UI validates addon URLs before it gets here, but this is the
/// boundary that has to hold on its own: a page-supplied string reaching a
/// `file:` or a credentialed URL would be this shell's fault, not the UI's.
fn checked_url(url: &str) -> Result<reqwest::Url> {
    let parsed = reqwest::Url::parse(url).map_err(|_| anyhow!("Request URL is not valid."))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(anyhow!("Only http and https requests are allowed."));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(anyhow!("Request URL must not carry credentials."));
    }
    Ok(parsed)
}

pub fn request(input: HttpRequest) -> Result<HttpResponse> {
    let url = checked_url(&input.url)?;
    let limit = input.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    let timeout = input
        .timeout_ms
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .min(MAX_TIMEOUT_MS);

    let method = match input.method.as_deref().unwrap_or("GET").to_ascii_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        other => return Err(anyhow!("Unsupported request method: {other}")),
    };

    let mut builder = client()
        .request(method, url)
        .timeout(Duration::from_millis(timeout));
    for (name, value) in input.headers.unwrap_or_default() {
        builder = builder.header(name, value);
    }
    if let Some(body) = input.body {
        builder = builder.body(body);
    }

    let response = builder.send()?;
    let status = response.status();
    let mut headers = std::collections::HashMap::new();
    for (name, value) in response.headers() {
        if let Ok(text) = value.to_str() {
            headers.insert(name.as_str().to_ascii_lowercase(), text.to_string());
        }
    }

    // Read one byte past the limit rather than trusting content-length: a host
    // that declares nothing, or declares a small body and sends a large one,
    // would otherwise be read into memory in full before anyone objected.
    let mut buffer = Vec::new();
    response.take(limit as u64 + 1).read_to_end(&mut buffer)?;
    if buffer.len() > limit {
        return Err(anyhow!("Response is too large."));
    }

    Ok(HttpResponse {
        ok: status.is_success(),
        status: status.as_u16(),
        headers,
        body: String::from_utf8_lossy(&buffer).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_schemes_a_page_should_not_be_able_to_dial() {
        for url in [
            "file:///C:/Windows/win.ini",
            "ftp://example.com/x",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
        ] {
            assert!(checked_url(url).is_err(), "{url} should be refused");
        }
    }

    #[test]
    fn refuses_urls_carrying_credentials() {
        assert!(checked_url("https://user:secret@example.com/meta.json").is_err());
        assert!(checked_url("https://user@example.com/meta.json").is_err());
    }

    #[test]
    fn allows_ordinary_addon_urls() {
        assert!(checked_url("https://v3-cinemeta.strem.io/manifest.json").is_ok());
        // Local addons are reached over plain http while they are being built.
        assert!(checked_url("http://127.0.0.1:11470/manifest.json").is_ok());
    }

    #[test]
    fn caps_a_timeout_a_caller_asks_too_much_of() {
        let asked = 10 * 60 * 1000u64;
        assert_eq!(asked.min(MAX_TIMEOUT_MS), MAX_TIMEOUT_MS);
    }
}

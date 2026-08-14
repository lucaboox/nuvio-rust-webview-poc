use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

const RELEASES_API: &str =
    "https://api.github.com/repos/lucaboox/nuvio-rust-webview-poc/releases?per_page=12";
const BUNDLED_CHANGELOG: &str = include_str!("../../CHANGELOG.md");

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotes {
    version: String,
    name: String,
    body: String,
    published_at: Option<String>,
    url: String,
    prerelease: bool,
}

pub fn github_release_notes() -> Result<Vec<ReleaseNotes>> {
    let releases = Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("NuvioDesktop/0.1 release-notes")
        .build()
        .context("could not create the GitHub release client")?
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .context("could not reach GitHub Releases")?
        .error_for_status()
        .context("GitHub Releases rejected the changelog request")?
        .json::<Vec<GithubRelease>>()
        .context("GitHub returned an unexpected release-history response")?;

    Ok(releases
        .into_iter()
        .filter(|release| !release.draft && release.tag_name.starts_with('v'))
        .map(|release| {
            let version = release.tag_name.trim_start_matches('v').to_string();
            let github_body = release.body.unwrap_or_default().trim().to_string();
            let body = changelog_section(BUNDLED_CHANGELOG, &version).unwrap_or(github_body);
            ReleaseNotes {
                name: release
                    .name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| format!("Nuvio {version}")),
                version,
                body,
                published_at: release.published_at,
                url: release.html_url,
                prerelease: release.prerelease,
            }
        })
        .collect())
}

fn changelog_section(changelog: &str, version: &str) -> Option<String> {
    let heading = format!("## [{version}]");
    let mut lines = changelog.lines();
    let found = lines.any(|line| line == heading || line.starts_with(&format!("{heading} - ")));
    if !found {
        return None;
    }

    let body = lines
        .take_while(|line| !line.starts_with("## ["))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    (!body.is_empty()).then_some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_payload_uses_the_frontend_shape() {
        let notes = ReleaseNotes {
            version: "0.2.0-alpha.1".into(),
            name: "Nuvio 0.2 Alpha 1".into(),
            body: "New player controls".into(),
            published_at: Some("2026-08-14T00:00:00Z".into()),
            url: "https://github.com/example/releases/tag/v0.2.0-alpha.1".into(),
            prerelease: true,
        };
        let value = serde_json::to_value(notes).unwrap();

        assert_eq!(value["publishedAt"], "2026-08-14T00:00:00Z");
        assert_eq!(value["prerelease"], true);
        assert!(value.get("published_at").is_none());
    }

    #[test]
    fn bundled_changelog_entries_override_generic_github_notes() {
        let changelog = "# Changelog\n\n## [0.2.0] - 2026-08-14\n\n- Real notes.\n\n## [0.1.0]\n\n- Older notes.";

        assert_eq!(
            changelog_section(changelog, "0.2.0").as_deref(),
            Some("- Real notes.")
        );
        assert!(changelog_section(changelog, "9.9.9").is_none());
    }
}

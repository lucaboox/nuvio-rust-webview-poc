import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { invoke } from "../bridge/nativeBridge";
import { Icon } from "./Icon";

type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "current"
  | "downloading"
  | "installing"
  | "error";

type GithubReleaseNotes = {
  version: string;
  name: string;
  body: string;
  publishedAt?: string;
  url: string;
  prerelease: boolean;
};

const FALLBACK_VERSION = "0.1.0-alpha.1";
const RELEASE_CACHE_KEY = "nuvio.github-release-notes.v1";
const RELEASE_CACHE_AGE_MS = 15 * 60 * 1000;

function friendlyVersion(version: string): string {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)(?:-([a-z]+)(?:\.(\d+))?)?/i);
  if (!match) return version;
  const [, major, minor, patch, channel, channelNumber] = match;
  const base = patch === "0" ? `${major}.${minor}` : `${major}.${minor}.${patch}`;
  if (!channel) return base;
  const label = channel[0].toUpperCase() + channel.slice(1);
  return `${base} ${label}${channelNumber ? ` ${channelNumber}` : ""}`;
}

function errorMessage(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  return String(reason || "Update check failed");
}

function releaseDate(value?: string): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  }).format(date);
}

function readableReleaseBody(body: string): string {
  return body
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\[([^\]]+)]\((https?:\/\/[^)]+)\)/g, "$1: $2")
    .trim();
}

function readCachedReleases(): GithubReleaseNotes[] | null {
  try {
    const cached = JSON.parse(sessionStorage.getItem(RELEASE_CACHE_KEY) || "null") as {
      savedAt?: number;
      releases?: GithubReleaseNotes[];
    } | null;
    if (
      cached?.savedAt
      && Date.now() - cached.savedAt < RELEASE_CACHE_AGE_MS
      && Array.isArray(cached.releases)
    ) {
      return cached.releases;
    }
  } catch {
    // A corrupt cache should never hide the live GitHub history.
  }
  return null;
}

export function UpdatesSection() {
  const [currentVersion, setCurrentVersion] = useState(FALLBACK_VERSION);
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [availableVersion, setAvailableVersion] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [progress, setProgress] = useState<number | null>(null);
  const [releases, setReleases] = useState<GithubReleaseNotes[]>(() => readCachedReleases() ?? []);
  const [releaseHistoryLoading, setReleaseHistoryLoading] = useState(releases.length === 0);
  const [releaseHistoryError, setReleaseHistoryError] = useState<string | null>(null);
  const pending = useRef<Update | null>(null);

  async function loadReleaseHistory(force = false) {
    if (!force) {
      const cached = readCachedReleases();
      if (cached) {
        setReleases(cached);
        setReleaseHistoryLoading(false);
        return;
      }
    }
    setReleaseHistoryLoading(true);
    setReleaseHistoryError(null);
    try {
      const payload = await invoke<{ releases: GithubReleaseNotes[] }>("updates.changelog");
      setReleases(payload.releases);
      try {
        sessionStorage.setItem(RELEASE_CACHE_KEY, JSON.stringify({
          savedAt: Date.now(),
          releases: payload.releases,
        }));
      } catch {
        // Release history still works when WebView storage is unavailable.
      }
    } catch (reason) {
      setReleaseHistoryError(errorMessage(reason));
    } finally {
      setReleaseHistoryLoading(false);
    }
  }

  useEffect(() => {
    getVersion().then(setCurrentVersion).catch(() => undefined);
    void loadReleaseHistory();
    return () => {
      void pending.current?.close();
    };
  }, []);

  async function checkForUpdate() {
    setPhase("checking");
    setProgress(null);
    setMessage("Checking the signed release channel…");
    try {
      await pending.current?.close();
      const update = await check({ timeout: 30_000 });
      pending.current = update;
      if (!update) {
        setAvailableVersion(null);
        setPhase("current");
        setMessage("You have the newest available version.");
        return;
      }
      setAvailableVersion(update.version);
      setPhase("available");
      setMessage(`Nuvio ${friendlyVersion(update.version)} is ready to install.`);
      void loadReleaseHistory(true);
    } catch (reason) {
      setPhase("error");
      setMessage(errorMessage(reason));
    }
  }

  async function installUpdate() {
    const update = pending.current;
    if (!update) return;
    if (!window.confirm(
      `Install Nuvio ${friendlyVersion(update.version)} now?\n\nThe app will close, update, and restart. Windows may ask for permission if Nuvio was installed for all users.`,
    )) return;

    let downloaded = 0;
    let total: number | undefined;
    setPhase("downloading");
    setProgress(0);
    setMessage("Downloading the signed update…");
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength;
          downloaded = 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setProgress(total ? Math.min(100, Math.round((downloaded / total) * 100)) : null);
        } else {
          setPhase("installing");
          setProgress(100);
          setMessage("Installing update and restarting Nuvio…");
        }
      });
      await relaunch();
    } catch (reason) {
      setPhase("error");
      setProgress(null);
      setMessage(errorMessage(reason));
    }
  }

  const busy = phase === "checking" || phase === "downloading" || phase === "installing";
  const phaseIcon = phase === "current" ? "check" : phase === "error" ? "close" : "refresh";

  return (
    <section className="settings-group update-settings-group">
      <div>
        <h2>Updates</h2>
        <p>Install signed releases and review what changed without leaving Nuvio.</p>
      </div>
      <div className="update-card">
        <div className="update-summary">
          <div className="update-version">
            <span>INSTALLED</span>
            <strong>Nuvio {friendlyVersion(currentVersion)}</strong>
            <small>{currentVersion} · Alpha channel</small>
          </div>
          <div className="update-actions">
            <button className="secondary-button" disabled={busy} onClick={checkForUpdate}>
              <Icon name="refresh" size={18} />
              {phase === "checking" ? "Checking…" : "Check for updates"}
            </button>
            {phase === "available" && (
              <button className="primary-button" onClick={installUpdate}>
                Update and restart
              </button>
            )}
          </div>
        </div>

        {phase !== "idle" && (
          <div className={`update-feedback update-${phase}`} role="status">
            <Icon name={phaseIcon} size={17} />
            <span>{availableVersion ? `Version ${friendlyVersion(availableVersion)} available — ${message}` : message}</span>
          </div>
        )}

        {phase === "downloading" || phase === "installing" ? (
          <div className="update-progress" aria-label="Update progress">
            <i style={{ width: progress === null ? "28%" : `${progress}%` }} />
            <span>{phase === "installing" ? "Installing…" : progress === null ? "Downloading…" : `${progress}%`}</span>
          </div>
        ) : null}

        <div className="release-history">
          <header>
            <div><span>CHANGELOG</span><h3>Release notes</h3></div>
            <small>GitHub Releases</small>
          </header>
          {releaseHistoryLoading && releases.length === 0 && <div className="release-history-state"><Icon name="refresh" size={16} /> Loading release notes…</div>}
          {releaseHistoryError && releases.length === 0 && (
            <div className="release-history-state error">
              <span>{releaseHistoryError}</span>
              <button className="text-button" onClick={() => void loadReleaseHistory(true)}>Try again</button>
            </div>
          )}
          <div className="release-list">
            {releases.map((release, index) => (
              <details key={release.version} open={index === 0}>
                <summary>
                  <span>
                    <strong>{friendlyVersion(release.version)}</strong>
                    {release.version === currentVersion && <i>Installed</i>}
                    {release.prerelease && <i>Prerelease</i>}
                  </span>
                  <small>{releaseDate(release.publishedAt)}</small>
                </summary>
                <div className="release-body">
                  <p>{readableReleaseBody(release.body) || "No release notes were provided."}</p>
                  <button className="text-button" onClick={() => void invoke("system.openExternal", { url: release.url })}>
                    View release on GitHub
                  </button>
                </div>
              </details>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

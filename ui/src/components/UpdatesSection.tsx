import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Icon } from "./Icon";

type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "current"
  | "downloading"
  | "installing"
  | "error";

const FALLBACK_VERSION = "0.1.0-alpha.1";

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

export function UpdatesSection() {
  const [currentVersion, setCurrentVersion] = useState(FALLBACK_VERSION);
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [availableVersion, setAvailableVersion] = useState<string | null>(null);
  const [notes, setNotes] = useState<string | null>(null);
  const [message, setMessage] = useState("Check GitHub Releases for a newer signed build.");
  const [progress, setProgress] = useState<number | null>(null);
  const pending = useRef<Update | null>(null);

  useEffect(() => {
    getVersion().then(setCurrentVersion).catch(() => undefined);
    return () => {
      void pending.current?.close();
    };
  }, []);

  async function checkForUpdate() {
    setPhase("checking");
    setProgress(null);
    setMessage("Checking the signed Nuvio release channel…");
    try {
      await pending.current?.close();
      const update = await check({ timeout: 30_000 });
      pending.current = update;
      if (!update) {
        setAvailableVersion(null);
        setNotes(null);
        setPhase("current");
        setMessage("You already have the newest available version.");
        return;
      }
      setAvailableVersion(update.version);
      setNotes(update.body?.trim() || null);
      setPhase("available");
      setMessage(`Nuvio ${friendlyVersion(update.version)} is ready to install.`);
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

  return (
    <section className="settings-group update-settings-group">
      <div>
        <h2>Application update</h2>
        <p>Updates are downloaded from GitHub Releases and verified before installation.</p>
      </div>
      <div className="update-card">
        <div className="update-version">
          <span>CURRENT VERSION</span>
          <strong>Nuvio {friendlyVersion(currentVersion)}</strong>
          <small>{currentVersion} · Alpha channel</small>
        </div>

        <div className={`update-status update-${phase}`} role="status">
          <Icon name={phase === "current" ? "check" : "refresh"} size={19} />
          <div>
            <strong>{availableVersion ? `Version ${friendlyVersion(availableVersion)} available` : message}</strong>
            {availableVersion && <span>{message}</span>}
          </div>
        </div>

        {notes && <div className="update-notes"><strong>What’s new</strong><p>{notes}</p></div>}
        {phase === "downloading" || phase === "installing" ? (
          <div className="update-progress" aria-label="Update progress">
            <i style={{ width: progress === null ? "28%" : `${progress}%` }} />
            <span>{phase === "installing" ? "Installing…" : progress === null ? "Downloading…" : `${progress}%`}</span>
          </div>
        ) : null}

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
    </section>
  );
}

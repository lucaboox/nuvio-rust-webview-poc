import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../bridge/nativeBridge";
import type { DownloadsSnapshot } from "../bridge/types";
import { Icon } from "./Icon";

export function DownloadSettingsSection() {
  const [snapshot, setSnapshot] = useState<DownloadsSnapshot | null>(null);
  const [selected, setSelected] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  function refresh() {
    invoke<DownloadsSnapshot>("downloads.list")
      .then((value) => { setSnapshot(value); if (!selected) setSelected(value.root); })
      .catch((reason) => setMessage(reason instanceof Error ? reason.message : "Download settings could not be loaded"));
  }
  useEffect(refresh, []);

  async function choose() {
    const value = await open({ directory: true, multiple: false, defaultPath: selected || snapshot?.root });
    if (typeof value === "string") setSelected(value);
  }

  async function move() {
    if (!selected || selected === snapshot?.root) return;
    if (!window.confirm("Move all completed downloads and cached artwork to this folder?")) return;
    setBusy(true); setMessage(null);
    try {
      await invoke("downloads.moveStorage", { path: selected });
      setMessage("Download storage moved successfully.");
      refresh();
    } catch (reason) {
      setMessage(reason instanceof Error ? reason.message : "Downloads could not be moved");
    } finally { setBusy(false); }
  }

  const active = snapshot?.items.filter((item) => item.status === "queued" || item.status === "downloading").length ?? 0;
  const complete = snapshot?.items.filter((item) => item.status === "completed").length ?? 0;
  return <section className="settings-group download-settings">
    <div><h2>Download location</h2><p>Video files, posters, and offline skip markers are stored locally on this computer.</p></div>
    <div className="download-location-card">
      <label><span>Current folder</span><div><input readOnly value={selected || snapshot?.root || "Loading…"} /><button className="secondary-button" disabled={busy} onClick={() => void choose()}><Icon name="external" size={16} />Choose</button></div></label>
      <div className="download-storage-summary"><span>{complete} downloaded</span><span>{active} active</span></div>
      <button className="primary-button" disabled={busy || !selected || selected === snapshot?.root} onClick={() => void move()}>{busy ? <i className="loading-spinner" /> : <Icon name="downloads" size={17} />}Move downloads</button>
      {active > 0 && <p className="download-location-warning">Finish or cancel active downloads before moving the folder.</p>}
      {message && <div className="inline-note">{message}</div>}
    </div>
  </section>;
}

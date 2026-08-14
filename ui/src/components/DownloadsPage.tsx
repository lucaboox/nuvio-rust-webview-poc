import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { DownloadItem, DownloadsSnapshot, StreamSource } from "../bridge/types";
import type { PlayContext } from "./DetailsOverlay";
import { Icon } from "./Icon";

export function DownloadsPage({ onPlay }: { onPlay(stream: StreamSource, context: PlayContext): void }) {
  const [snapshot, setSnapshot] = useState<DownloadsSnapshot | null>(null);
  const [filter, setFilter] = useState<"all" | "active" | "completed">("all");
  const [artwork, setArtwork] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [speeds, setSpeeds] = useState<Record<string, number>>({});
  const speedSamples = useRef(new Map<string, { bytes: number; sampledAt: number; speed: number }>());

  const refresh = useCallback(() => {
    invoke<DownloadsSnapshot>("downloads.list")
      .then((value) => {
        const sampledAt = performance.now();
        const nextSpeeds: Record<string, number> = {};
        const activeIds = new Set<string>();
        for (const item of value.items) {
          if (item.status !== "downloading") continue;
          activeIds.add(item.id);
          const previous = speedSamples.current.get(item.id);
          let speed = previous?.speed ?? 0;
          if (previous && sampledAt > previous.sampledAt && item.bytesDownloaded >= previous.bytes) {
            const measured = (item.bytesDownloaded - previous.bytes) * 1000 / (sampledAt - previous.sampledAt);
            speed = previous.speed > 0 ? previous.speed * 0.65 + measured * 0.35 : measured;
          }
          speedSamples.current.set(item.id, { bytes: item.bytesDownloaded, sampledAt, speed });
          nextSpeeds[item.id] = speed;
        }
        for (const id of speedSamples.current.keys()) {
          if (!activeIds.has(id)) speedSamples.current.delete(id);
        }
        setSpeeds(nextSpeeds);
        setSnapshot(value);
        setError(null);
      })
      .catch((reason) => setError(reason instanceof Error ? reason.message : "Downloads could not be loaded"));
  }, []);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, snapshot?.items.some((item) => item.status === "downloading" || item.status === "queued") ? 750 : 3000);
    return () => window.clearInterval(timer);
  }, [refresh, snapshot?.items.some((item) => item.status === "downloading" || item.status === "queued")]);

  useEffect(() => {
    for (const item of snapshot?.items ?? []) {
      if (!item.artworkCached || artwork[item.id]) continue;
      invoke<{ image?: string }>("downloads.artwork", { id: item.id })
        .then((result) => { if (result.image) setArtwork((current) => ({ ...current, [item.id]: result.image! })); })
        .catch(() => undefined);
    }
  }, [snapshot?.items, artwork]);

  const visible = useMemo(() => (snapshot?.items ?? []).filter((item) =>
    filter === "all" || (filter === "active" ? item.status === "queued" || item.status === "downloading" : item.status === "completed")
  ), [snapshot?.items, filter]);

  async function action(method: string, id: string) {
    try { await invoke(method, { id }); refresh(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "Download action failed"); }
  }

  function play(item: DownloadItem) {
    if (!item.playUrl) return;
    const stream: StreamSource = {
      name: "Offline download", title: item.title, description: item.sourceName,
      url: item.playUrl, sources: [], addonName: "Downloads", addonId: "local.downloads",
      behaviorHints: { notWebReady: false, filename: item.filePath },
    };
    onPlay(stream, {
      title: item.title,
      showName: item.showName,
      contentId: item.contentId,
      contentType: item.contentType,
      videoId: item.videoId,
      season: item.season,
      episode: item.episode,
      startPositionMs: 0,
      offline: true,
    });
  }

  return <div className="downloads-page">
    <div className="feature-title downloads-title"><div><span>OFFLINE</span><h1>Downloads</h1><p>{snapshot?.root || "Loading download storage…"}</p></div><button className="secondary-button" onClick={() => void invoke("downloads.openFolder")}><Icon name="external" size={17} />Open folder</button></div>
    <div className="downloads-tabs"><button className={filter === "all" ? "active" : undefined} onClick={() => setFilter("all")}>All</button><button className={filter === "active" ? "active" : undefined} onClick={() => setFilter("active")}>Downloading</button><button className={filter === "completed" ? "active" : undefined} onClick={() => setFilter("completed")}>Downloaded</button></div>
    {error && <div className="inline-error">{error}</div>}
    {!snapshot ? <div className="downloads-empty"><i className="loading-spinner" /><strong>Loading downloads</strong></div> : visible.length === 0 ? <div className="downloads-empty"><Icon name="downloads" size={42} /><strong>No {filter === "all" ? "downloads" : filter} yet</strong><span>Right-click a playable source to save it for offline viewing.</span></div> : <div className="download-list">{visible.map((item) => <DownloadRow key={item.id} item={item} image={artwork[item.id]} speed={speeds[item.id]} onPlay={() => play(item)} onCancel={() => void action("downloads.cancel", item.id)} onRetry={() => void action("downloads.retry", item.id)} onRemove={() => void action("downloads.remove", item.id)} />)}</div>}
  </div>;
}

function DownloadRow({ item, image, speed, onPlay, onCancel, onRetry, onRemove }: { item: DownloadItem; image?: string; speed?: number; onPlay(): void; onCancel(): void; onRetry(): void; onRemove(): void }) {
  const percent = item.totalBytes ? Math.min(100, item.bytesDownloaded / item.totalBytes * 100) : 0;
  const episode = item.season != null && item.episode != null ? `S${item.season} E${item.episode}` : item.contentType;
  return <article className="download-row">
    <div className="download-art">{image ? <img src={image} alt="" /> : <Icon name="video" size={30} />}</div>
    <div className="download-copy"><small>{episode} · {item.status}</small><h2>{item.showName || item.title}</h2>{item.showName && <strong>{item.title}</strong>}<span>{item.sourceName || item.filePath || "Nuvio download"}</span>{(item.status === "downloading" || item.status === "queued") && <div className="download-progress"><i><b style={{ width: `${percent}%` }} /></i><span>{item.status === "queued" ? "Queued" : `${formatBytes(item.bytesDownloaded)}${item.totalBytes ? ` of ${formatBytes(item.totalBytes)}` : ""} · ${speed && speed > 0 ? formatSpeed(speed) : "Measuring…"}`}</span></div>}{item.error && <em>{item.error}</em>}{item.skipSegments.length > 0 && <label><Icon name="check" size={13} />Offline skip markers cached</label>}</div>
    <div className="download-actions">{item.status === "completed" && <button className="primary-button" onClick={onPlay}><Icon name="play" size={17} />Play</button>}{(item.status === "queued" || item.status === "downloading") && <button className="secondary-button" onClick={onCancel}>Cancel</button>}{(item.status === "failed" || item.status === "cancelled") && <button className="secondary-button" onClick={onRetry}><Icon name="refresh" size={16} />Retry</button>}{item.status !== "downloading" && item.status !== "queued" && <button className="icon-pill" title="Remove download" aria-label="Remove download" onClick={onRemove}><Icon name="trash" size={17} /></button>}</div>
  </article>;
}

function formatBytes(bytes: number) {
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(0)} MB`;
  return `${Math.max(0, Math.round(bytes / 1024))} KB`;
}

function formatSpeed(bytesPerSecond: number) {
  if (bytesPerSecond >= 1_073_741_824) return `${(bytesPerSecond / 1_073_741_824).toFixed(1)} GB/s`;
  if (bytesPerSecond >= 1_048_576) return `${(bytesPerSecond / 1_048_576).toFixed(1)} MB/s`;
  return `${Math.max(0, bytesPerSecond / 1024).toFixed(0)} KB/s`;
}

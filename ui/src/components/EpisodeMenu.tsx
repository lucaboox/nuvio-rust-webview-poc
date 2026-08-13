import { useEffect, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { ContentMeta, Video } from "../bridge/types";
import { Icon } from "./Icon";
import { clearWatchedOverride, setWatchedOverride, watchedKey } from "../data/watchedOverrides";

const openEvent = "nuvio-episode-context-menu";

export type EpisodeMenuTarget = {
  details: Pick<ContentMeta, "id" | "contentType" | "name">;
  video: Video;
  watched: boolean;
  x: number;
  y: number;
};

export function showEpisodeContextMenu(
  event: React.MouseEvent,
  target: Omit<EpisodeMenuTarget, "x" | "y">,
) {
  event.preventDefault();
  event.stopPropagation();
  window.dispatchEvent(
    new CustomEvent<EpisodeMenuTarget>(openEvent, {
      detail: { ...target, x: event.clientX, y: event.clientY },
    }),
  );
}

/** Right-click actions for an episode: mark watched, or reset it entirely. */
export function EpisodeContextMenu({ onChanged }: { onChanged(): void }) {
  const [menu, setMenu] = useState<EpisodeMenuTarget | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const open = (event: Event) => {
      setMenu((event as CustomEvent<EpisodeMenuTarget>).detail);
      setBusy(false);
    };
    const dismiss = () => setMenu(null);
    window.addEventListener(openEvent, open);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    window.addEventListener("scroll", dismiss, true);
    return () => {
      window.removeEventListener(openEvent, open);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("scroll", dismiss, true);
    };
  }, []);

  if (!menu) return null;
  const left = Math.max(10, Math.min(menu.x, window.innerWidth - 250));
  const top = Math.max(10, Math.min(menu.y, window.innerHeight - 160));

  async function run(method: string, params: unknown, optimistic?: { key: string; watched: boolean }) {
    if (busy) return;
    setBusy(true);
    // Flip the row now; the store releases the override once the refreshed
    // snapshot agrees, or we roll it back if the write fails.
    if (optimistic) setWatchedOverride(optimistic.key, optimistic.watched);
    setMenu(null);
    try {
      await invoke(method, params);
      onChanged();
    } catch {
      if (optimistic) clearWatchedOverride(optimistic.key);
    } finally {
      setBusy(false);
    }
  }

  const identity = {
    contentId: menu.details.id,
    contentType: menu.details.contentType,
    videoId: menu.video.id,
    season: menu.video.season,
    episode: menu.video.episode,
  };

  return (
    <>
      <button className="title-menu-dismiss" aria-label="Close menu" onClick={() => setMenu(null)} />
      <div className="title-context-menu" style={{ left, top }} role="menu">
        <div className="title-context-heading">
          <strong>{menu.video.title || `Episode ${menu.video.episode ?? 1}`}</strong>
          <span>S{menu.video.season ?? 0} E{menu.video.episode ?? 1}</span>
        </div>
        <button
          role="menuitem"
          disabled={busy}
          onClick={() =>
            run(
              "progress.setWatched",
              { identity, title: menu.details.name, watched: !menu.watched },
              { key: watchedKey(menu.details.id, menu.video.season, menu.video.episode), watched: !menu.watched },
            )
          }
        >
          <Icon name={menu.watched ? "close" : "check"} size={18} />
          <span>{menu.watched ? "Mark as not watched" : "Mark as watched"}</span>
        </button>
        <button
          role="menuitem"
          className="menu-destructive"
          disabled={busy}
          onClick={() =>
            run(
              "progress.setWatched",
              { identity, title: menu.details.name, watched: false },
              { key: watchedKey(menu.details.id, menu.video.season, menu.video.episode), watched: false },
            )
          }
        >
          <Icon name="refresh" size={18} />
          <span>Reset progress</span>
        </button>
      </div>
    </>
  );
}

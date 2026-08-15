import { useEffect, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { ContentMeta } from "../bridge/types";
import { Icon } from "./Icon";
import { isInLibrary, setLibraryMembership } from "../data/libraryCache";

const openEvent = "nuvio-title-context-menu";
const libraryChangedEvent = "nuvio-library-changed";

/** Identifies a Continue Watching card so it can be dismissed. */
export type DismissTarget = {
  contentId: string;
  season?: number;
  episode?: number;
};

type MenuRequest = {
  item: ContentMeta;
  x: number;
  y: number;
  savedHint?: boolean;
  dismiss?: DismissTarget;
};

export function showTitleContextMenu(
  event: React.MouseEvent,
  item: ContentMeta,
  savedHint?: boolean,
  dismiss?: DismissTarget,
) {
  event.preventDefault();
  event.stopPropagation();
  window.dispatchEvent(
    new CustomEvent<MenuRequest>(openEvent, {
      detail: { item, x: event.clientX, y: event.clientY, savedHint, dismiss },
    }),
  );
}

export function TitleContextMenu({
  onSelect,
}: {
  onSelect(item: ContentMeta): void;
}) {
  const [menu, setMenu] = useState<MenuRequest | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const open = (event: Event) => {
      const detail = (event as CustomEvent<MenuRequest>).detail;
      setMenu(detail);
      setSaved(detail.savedHint ?? isInLibrary(detail.item));
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
  const left = Math.max(10, Math.min(menu.x, window.innerWidth - 245));
  const top = Math.max(10, Math.min(menu.y, window.innerHeight - 190));
  async function toggleLibrary() {
    if (!menu || busy) return;
    setBusy(true);
    try {
      if (saved)
        await invoke("library.remove", {
          type: menu.item.contentType,
          id: menu.item.id,
        });
      else await invoke("library.add", { item: menu.item });
      setLibraryMembership(menu.item, !saved);
      setSaved(!saved);
      window.dispatchEvent(new Event(libraryChangedEvent));
      setMenu(null);
    } catch {
      setMenu(null);
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <button
        className="title-menu-dismiss"
        aria-label="Close title menu"
        onClick={() => setMenu(null)}
      />
      <div className="title-context-menu" style={{ left, top }} role="menu">
        <div className="title-context-heading">
          <strong>{menu.item.name}</strong>
          <span>{menu.item.releaseInfo || menu.item.contentType}</span>
        </div>
        <button
          role="menuitem"
          onClick={() => {
            onSelect(menu.item);
            setMenu(null);
          }}
        >
          <Icon name="info" size={18} />
          <span>View details</span>
        </button>
        <button
          role="menuitem"
          className={saved ? "menu-destructive" : undefined}
          disabled={busy}
          onClick={toggleLibrary}
        >
          <Icon name={saved ? "close" : "plus"} size={18} />
          <span>{saved ? "Remove from library" : "Add to library"}</span>
        </button>
        <button
          role="menuitem"
          onClick={() => {
            void navigator.clipboard.writeText(menu.item.name);
            setMenu(null);
          }}
        >
          <Icon name="copy" size={18} />
          <span>Copy title</span>
        </button>
        {menu.dismiss && (
          <button
            role="menuitem"
            className="menu-destructive"
            disabled={busy}
            onClick={() => {
              const target = menu.dismiss!;
              setMenu(null);
              // Records Nuvio's nextUpDismissKey in the synced continue
              // watching preferences, so the card stays hidden everywhere.
              void invoke("continueWatching.dismiss", {
                contentId: target.contentId,
                season: target.season ?? null,
                episode: target.episode ?? null,
                dismissed: true,
              }).then(() => window.dispatchEvent(new Event(libraryChangedEvent)));
            }}
          >
            <Icon name="close" size={18} />
            <span>Dismiss from Continue Watching</span>
          </button>
        )}
      </div>
    </>
  );
}

export function onLibraryChanged(callback: () => void) {
  window.addEventListener(libraryChangedEvent, callback);
  return () => window.removeEventListener(libraryChangedEvent, callback);
}

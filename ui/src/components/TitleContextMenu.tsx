import { useEffect, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { ContentMeta, LibraryItem } from "../bridge/types";
import { Icon } from "./Icon";

const openEvent = "nuvio-title-context-menu";
const libraryChangedEvent = "nuvio-library-changed";

type MenuRequest = {
  item: ContentMeta;
  x: number;
  y: number;
  savedHint?: boolean;
};

export function showTitleContextMenu(
  event: React.MouseEvent,
  item: ContentMeta,
  savedHint?: boolean,
) {
  event.preventDefault();
  event.stopPropagation();
  window.dispatchEvent(
    new CustomEvent<MenuRequest>(openEvent, {
      detail: { item, x: event.clientX, y: event.clientY, savedHint },
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
      setSaved(detail.savedHint ?? false);
      setBusy(false);
      if (detail.savedHint == null) {
        invoke<{ items: LibraryItem[] }>("library.list")
          .then((result) =>
            setSaved(
              result.items.some(
                (item) =>
                  item.id === detail.item.id &&
                  item.contentType === detail.item.contentType,
              ),
            ),
          )
          .catch(() => undefined);
      }
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
        <button role="menuitem" disabled={busy} onClick={toggleLibrary}>
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
      </div>
    </>
  );
}

export function onLibraryChanged(callback: () => void) {
  window.addEventListener(libraryChangedEvent, callback);
  return () => window.removeEventListener(libraryChangedEvent, callback);
}

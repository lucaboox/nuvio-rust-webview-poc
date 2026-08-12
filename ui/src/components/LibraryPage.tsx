import { useEffect, useMemo, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type {
  ContentMeta,
  LibraryItem,
  ProgressSnapshot,
} from "../bridge/types";
import { WatchStatus, watchStateForContent } from "./WatchStatus";
import { showTitleContextMenu } from "./TitleContextMenu";

type LibraryFilter = "all" | "movies" | "shows";

export function LibraryPage({
  profileIndex,
  revision,
  progress,
  onSelect,
}: {
  profileIndex: number;
  revision: number;
  progress: ProgressSnapshot;
  onSelect(item: ContentMeta): void;
}) {
  const [items, setItems] = useState<LibraryItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<LibraryFilter>("all");
  useEffect(() => {
    setItems(null);
    setError(null);
    invoke<{ items: LibraryItem[] }>("library.list")
      .then((result) => setItems(result.items))
      .catch((reason: Error) => setError(reason.message));
  }, [profileIndex, revision]);
  const visibleItems = useMemo(
    () =>
      (items ?? []).filter(
        (item) =>
          filter === "all" ||
          (filter === "movies"
            ? isMovie(item.contentType)
            : !isMovie(item.contentType)),
      ),
    [items, filter],
  );
  return (
    <div className="library-page">
      <div className="feature-title library-title">
        <div>
          <span>SYNCED LIBRARY</span>
          <h1>My library</h1>
          <p>Movies and series saved to your active Nuvio profile.</p>
        </div>
        <div className="library-tabs" role="tablist" aria-label="Library type">
          {(["all", "movies", "shows"] as LibraryFilter[]).map((value) => (
            <button
              role="tab"
              aria-selected={filter === value}
              className={filter === value ? "active" : ""}
              key={value}
              onClick={() => setFilter(value)}
            >
              {value[0].toUpperCase() + value.slice(1)}
            </button>
          ))}
        </div>
      </div>
      {error ? (
        <div className="inline-error">{error}</div>
      ) : !items ? (
        <div className="library-loading">
          <i className="loading-spinner" /> Loading library…
        </div>
      ) : items.length === 0 ? (
        <div className="empty-feature">
          <strong>Your library is empty</strong>
          <span>Add a movie or series from its details page.</span>
        </div>
      ) : visibleItems.length === 0 ? (
        <div className="empty-feature">
          <strong>No {filter} saved</strong>
          <span>Try another library tab.</span>
        </div>
      ) : (
        <div className="library-grid">
          {visibleItems.map((item) => (
            <button
              key={`${item.contentType}:${item.id}`}
              onClick={() => onSelect(item)}
              onContextMenu={(event) => showTitleContextMenu(event, item, true)}
            >
              <div
                className="library-poster"
                style={
                  item.poster
                    ? {
                        backgroundImage: `url("${item.poster.replaceAll('"', "%22")}")`,
                      }
                    : undefined
                }
              >
                {!item.poster && <strong>{item.name}</strong>}
                <WatchStatus state={watchStateForContent(item, progress)} />
              </div>
              <strong>{item.name}</strong>
              <span>{item.releaseInfo || item.contentType}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function isMovie(type: string) {
  return type.toLowerCase() === "movie" || type.toLowerCase() === "film";
}

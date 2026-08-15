import { typeLabel } from "../data/catalogLabels";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type {
  CatalogSection,
  ContentMeta,
  DiscoverCatalog,
  ProgressSnapshot,
} from "../bridge/types";
import { WatchStatus, watchStateForContent } from "./WatchStatus";
import { showTitleContextMenu } from "./TitleContextMenu";

const ALL_GENRES = "__all__";

/**
 * Browse addon catalogs without a search term, mirroring Nuvio's Discover:
 * a type picker, the catalogs that serve that type, and the genres that
 * catalog's manifest advertises.
 */
export function DiscoverPage({
  progress,
  addonSignature,
  onSelect,
}: {
  progress: ProgressSnapshot;
  addonSignature: string;
  onSelect(item: ContentMeta): void;
}) {
  const [catalogs, setCatalogs] = useState<DiscoverCatalog[] | null>(null);
  const [type, setType] = useState<string | null>(null);
  const [catalogKey, setCatalogKey] = useState<string | null>(null);
  const [genre, setGenre] = useState<string>(ALL_GENRES);
  const [items, setItems] = useState<ContentMeta[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [canLoadMore, setCanLoadMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sentinel = useRef<HTMLDivElement | null>(null);
  const busyRef = useRef(false);

  useEffect(() => {
    invoke<{ catalogs: DiscoverCatalog[] }>("content.discoverCatalogs")
      .then((result) => setCatalogs(result.catalogs))
      .catch((reason: Error) => {
        setCatalogs([]);
        setError(reason.message);
      });
  }, [addonSignature]);

  const types = useMemo(
    // `catalogs` already follows installed-addon priority and each manifest's
    // catalog order. Set preserves that first-seen order; sorting here made the
    // picker disagree with Nuvio and the user's addon configuration.
    () => [...new Set((catalogs ?? []).map((item) => item.contentType))],
    [catalogs],
  );
  const activeType = type ?? types[0] ?? null;
  const typeCatalogs = useMemo(
    () => (catalogs ?? []).filter((item) => item.contentType === activeType),
    [catalogs, activeType],
  );
  const catalog =
    typeCatalogs.find((item) => item.key === catalogKey) ?? typeCatalogs[0];

  // Nuvio's resolveGenreSelection: a required genre falls back to the first
  // option, an optional one to "all".
  const effectiveGenre = useMemo(() => {
    if (!catalog || catalog.genreOptions.length === 0) return undefined;
    if (genre !== ALL_GENRES && catalog.genreOptions.includes(genre))
      return genre;
    return catalog.genreRequired ? catalog.genreOptions[0] : undefined;
  }, [catalog, genre]);

  const load = useCallback(
    async (skip: number) => {
      if (!catalog) return;
      if (skip === 0) {
        setLoading(true);
        setItems([]);
      } else {
        setLoadingMore(true);
      }
      setError(null);
      try {
        const section = await invoke<CatalogSection>("content.catalog", {
          manifestUrl: catalog.manifestUrl,
          type: catalog.contentType,
          catalogId: catalog.catalogId,
          genre: effectiveGenre,
          skip,
        });
        setItems((current) => {
          if (skip === 0) return section.items;
          const known = new Set(
            current.map((item) => `${item.contentType}:${item.id}`),
          );
          const additions = section.items.filter(
            (item) => !known.has(`${item.contentType}:${item.id}`),
          );
          setCanLoadMore(catalog.supportsPagination && additions.length > 0);
          return [...current, ...additions];
        });
        if (skip === 0)
          setCanLoadMore(
            catalog.supportsPagination && section.items.length > 0,
          );
      } catch (reason) {
        setError(
          reason instanceof Error ? reason.message : "Could not load catalog",
        );
        setCanLoadMore(false);
      } finally {
        setLoading(false);
        setLoadingMore(false);
        busyRef.current = false;
      }
    },
    [catalog, effectiveGenre],
  );

  useEffect(() => {
    if (catalog) void load(0);
  }, [catalog?.key, effectiveGenre, load]);

  useEffect(() => {
    const target = sentinel.current;
    if (!target || loading || !canLoadMore) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        if (busyRef.current) return;
        busyRef.current = true;
        void load(items.length);
      },
      { rootMargin: "700px 0px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [canLoadMore, items.length, load, loading]);

  if (!catalogs)
    return (
      <div className="catalog-page">
        <div className="loading-page">
          <span className="loading-spinner" />
          <strong>Reading catalogs from your addons…</strong>
        </div>
      </div>
    );

  if (catalogs.length === 0)
    return (
      <div className="feature-page">
        <div className="feature-title">
          <div>
            <span>DISCOVER</span>
            <h1>Browse catalogs</h1>
          </div>
        </div>
        <div className="empty-feature">
          <strong>No browsable catalogs</strong>
          <span>
            None of your enabled addons expose a catalog that can be browsed
            without a search term.
          </span>
        </div>
      </div>
    );

  return (
    <div className="catalog-page discover-page">
      <div className="feature-title">
        <div>
          <span>DISCOVER</span>
          <h1>Browse catalogs</h1>
          <p>
            {catalog
              ? `${catalog.catalogName} · ${catalog.addonName}${items.length ? ` · ${items.length} titles` : ""}`
              : "Pick a catalog to browse."}
          </p>
        </div>
      </div>

      <div className="discover-filters">
        <label>
          <span>Type</span>
          <select
            value={activeType ?? ""}
            onChange={(event) => {
              setType(event.target.value);
              setCatalogKey(null);
              setGenre(ALL_GENRES);
            }}
          >
            {types.map((option) => (
              <option key={option} value={option}>
                {typeLabel(option)}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Catalog</span>
          <select
            value={catalog?.key ?? ""}
            onChange={(event) => {
              setCatalogKey(event.target.value);
              setGenre(ALL_GENRES);
            }}
          >
            {typeCatalogs.map((option) => (
              <option key={option.key} value={option.key}>
                {option.catalogName}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Genre</span>
          <select
            value={effectiveGenre ?? ALL_GENRES}
            disabled={!catalog || catalog.genreOptions.length === 0}
            onChange={(event) => setGenre(event.target.value)}
          >
            {catalog && !catalog.genreRequired && (
              <option value={ALL_GENRES}>All genres</option>
            )}
            {catalog?.genreOptions.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
            {catalog?.genreOptions.length === 0 && (
              <option value={ALL_GENRES}>Not supported</option>
            )}
          </select>
        </label>
      </div>

      {error && <div className="inline-error catalog-error">{error}</div>}

      {loading ? (
        <div className="loading-page">
          <span className="loading-spinner" />
          <strong>Loading {catalog?.catalogName}…</strong>
        </div>
      ) : items.length === 0 && !error ? (
        <div className="empty-feature">
          <strong>Nothing returned</strong>
          <span>This catalog produced no titles for that filter.</span>
        </div>
      ) : (
        <div className="catalog-grid">
          {items.map((item) => (
            <button
              key={`${item.sourceManifestUrl}:${item.id}`}
              onClick={() => onSelect(item)}
              onContextMenu={(event) => showTitleContextMenu(event, item)}
            >
              <div
                className="catalog-poster"
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

      {!loading && canLoadMore && (
        <div className="catalog-load-sentinel" ref={sentinel}>
          {loadingMore && (
            <>
              <i className="loading-spinner" />
              Loading more titles…
            </>
          )}
        </div>
      )}
    </div>
  );
}


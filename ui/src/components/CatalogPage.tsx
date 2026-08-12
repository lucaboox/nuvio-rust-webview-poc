import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { CatalogSection, ContentMeta, ProgressSnapshot } from "../bridge/types";
import { Icon } from "./Icon";
import { WatchStatus, watchStateForContent } from "./WatchStatus";
import { showTitleContextMenu } from "./TitleContextMenu";

export function CatalogPage({ source, progress, onBack, onSelect }: { source: CatalogSection; progress: ProgressSnapshot; onBack(): void; onSelect(item: ContentMeta): void }) {
  const [section, setSection] = useState(source);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [canLoadMore, setCanLoadMore] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const loadSentinel = useRef<HTMLDivElement | null>(null);
  const loadingMoreRef = useRef(false);

  useEffect(() => {
    setLoading(true); setError(null);
    invoke<CatalogSection>("content.catalog", { manifestUrl: source.manifestUrl, type: source.contentType, catalogId: source.catalogId, genre: source.genre, skip: 0 })
      .then((result) => { setSection(result); setCanLoadMore(result.items.length > 0); })
      .catch((reason: Error) => setError(reason.message)).finally(() => setLoading(false));
  }, [source]);

  const loadMore = useCallback(async () => {
    if (loading || loadingMoreRef.current || !canLoadMore) return;
    loadingMoreRef.current = true;
    setLoadingMore(true); setError(null);
    try {
      const result = await invoke<CatalogSection>("content.catalog", { manifestUrl: source.manifestUrl, type: source.contentType, catalogId: source.catalogId, genre: source.genre, skip: section.items.length });
      const known = new Set(section.items.map((item) => `${item.contentType}:${item.id}`));
      const additions = result.items.filter((item) => !known.has(`${item.contentType}:${item.id}`));
      setSection((current) => ({ ...current, items: [...current.items, ...additions] }));
      setCanLoadMore(result.items.length > 0 && additions.length > 0);
    } catch (reason) { setError(reason instanceof Error ? reason.message : "Could not load more titles"); }
    finally { loadingMoreRef.current = false; setLoadingMore(false); }
  }, [canLoadMore, loading, section.items, source]);

  useEffect(() => {
    const target = loadSentinel.current;
    if (!target || loading || !canLoadMore) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void loadMore();
    }, { rootMargin: "700px 0px" });
    observer.observe(target);
    return () => observer.disconnect();
  }, [canLoadMore, loadMore, loading]);

  return <div className="catalog-page">
    <div className="catalog-header"><button className="round-back-button" aria-label="Back" title="Back" onClick={onBack}><Icon name="back" size={25} /></button><div><span>{source.subtitle}</span><h1>{source.title}</h1><p>{loading ? "Loading catalog…" : `${section.items.length} titles loaded`}</p></div></div>
    {error && <div className="inline-error catalog-error">{error}</div>}
    <div className="catalog-grid">{section.items.map((item) => <button key={`${item.sourceManifestUrl}:${item.id}`} onClick={() => onSelect(item)} onContextMenu={(event) => showTitleContextMenu(event, item)}><div className="catalog-poster" style={item.poster ? { backgroundImage: `url("${item.poster.replaceAll('"', '%22')}")` } : undefined}>{!item.poster && <strong>{item.name}</strong>}<WatchStatus state={watchStateForContent(item, progress)} /></div><strong>{item.name}</strong><span>{item.releaseInfo || item.contentType}</span></button>)}</div>
    {!loading && canLoadMore && <div className="catalog-load-sentinel" ref={loadSentinel}>{loadingMore && <><i className="loading-spinner" />Loading more titles…</>}</div>}
  </div>;
}

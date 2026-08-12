import { useEffect, useMemo, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type {
  CatalogSection,
  CollectionCatalogSource,
  CollectionFolder,
  ContentMeta,
  HomePayload,
  NuvioCollection,
  ProgressSnapshot,
} from "../bridge/types";
import { Icon } from "./Icon";
import { MediaRow } from "./MediaRow";
import { WatchStatus, watchStateForContent } from "./WatchStatus";
import { showTitleContextMenu } from "./TitleContextMenu";

export function CollectionSettingsSection({
  collections,
  loading,
  error,
  onRefresh,
  onReorder,
  onFolder,
}: {
  collections: NuvioCollection[];
  loading: boolean;
  error: string | null;
  onRefresh(): void;
  onReorder(
    collectionId: string,
    folderId: string | undefined,
    direction: -1 | 1,
  ): void;
  onFolder(collection: NuvioCollection, folder: CollectionFolder): void;
}) {
  return (
    <section className="settings-group collection-settings-group">
      <div>
        <h2>Collections</h2>
        <p>
          Organize the collections and folders shown on Home. Changes sync with
          Nuvio.
        </p>
        <button
          className="collection-settings-refresh"
          onClick={onRefresh}
          disabled={loading}
        >
          <Icon name="refresh" size={17} />
          Refresh
        </button>
      </div>
      <div className="settings-list collection-settings-list">
        {error ? (
          <div className="inline-error">{error}</div>
        ) : loading ? (
          <div className="library-loading">
            <i className="loading-spinner" /> Syncing collections…
          </div>
        ) : collections.length === 0 ? (
          <div className="collection-settings-empty">No synced collections</div>
        ) : (
          collections.map((collection, collectionIndex) => (
            <article className="collection-settings-card" key={collection.id}>
              <header>
                <div>
                  <strong>{collection.title}</strong>
                  <span>
                    {collection.folders.length} folder
                    {collection.folders.length === 1 ? "" : "s"}
                  </span>
                </div>
                <div className="reorder-buttons">
                  <button
                    aria-label={`Move ${collection.title} up`}
                    disabled={collectionIndex === 0}
                    onClick={() => onReorder(collection.id, undefined, -1)}
                  >
                    <Icon name="up" size={18} />
                  </button>
                  <button
                    aria-label={`Move ${collection.title} down`}
                    disabled={collectionIndex === collections.length - 1}
                    onClick={() => onReorder(collection.id, undefined, 1)}
                  >
                    <Icon name="down" size={18} />
                  </button>
                </div>
              </header>
              <div className="collection-settings-folders">
                {collection.folders.map((folder, folderIndex) => (
                  <div key={folder.id}>
                    <button
                      className="collection-folder-open"
                      onClick={() => onFolder(collection, folder)}
                    >
                      <span>{folder.coverEmoji || "▦"}</span>
                      <strong>{folder.title}</strong>
                    </button>
                    <div className="reorder-buttons">
                      <button
                        aria-label={`Move ${folder.title} up`}
                        disabled={folderIndex === 0}
                        onClick={() => onReorder(collection.id, folder.id, -1)}
                      >
                        <Icon name="up" size={17} />
                      </button>
                      <button
                        aria-label={`Move ${folder.title} down`}
                        disabled={folderIndex === collection.folders.length - 1}
                        onClick={() => onReorder(collection.id, folder.id, 1)}
                      >
                        <Icon name="down" size={17} />
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </article>
          ))
        )}
      </div>
    </section>
  );
}

export function CollectionRows({
  collections,
  onFolder,
}: {
  collections: NuvioCollection[];
  onFolder(collection: NuvioCollection, folder: CollectionFolder): void;
}) {
  return (
    <>
      {collections.map(
        (collection) =>
          collection.folders.length > 0 && (
            <section
              className="media-section collection-section"
              key={collection.id}
            >
              <div className="section-heading">
                <div>
                  <h2>{collection.title}</h2>
                  <p>
                    {collection.folders.length} folder
                    {collection.folders.length === 1 ? "" : "s"} · Synced from
                    Nuvio
                  </p>
                </div>
              </div>
              <div className="media-row">
                {collection.folders.map((folder) => (
                  <button
                    className={`collection-folder-card shape-${normalizedShape(folder.tileShape)}`}
                    key={folder.id}
                    onClick={() => onFolder(collection, folder)}
                  >
                    <div
                      className="collection-folder-art"
                      style={
                        folderImage(folder)
                          ? {
                              backgroundImage: `url("${folderImage(folder)?.replaceAll('"', "%22")}")`,
                            }
                          : undefined
                      }
                    >
                      {!folderImage(folder) && (
                        <span>
                          {folder.coverEmoji ||
                            folder.title.slice(0, 2).toUpperCase()}
                        </span>
                      )}
                    </div>
                    {!folder.hideTitle && <strong>{folder.title}</strong>}
                  </button>
                ))}
              </div>
            </section>
          ),
      )}
    </>
  );
}

export function CollectionsPage({
  collections,
  loading,
  error,
  onRefresh,
  onFolder,
}: {
  collections: NuvioCollection[];
  loading: boolean;
  error: string | null;
  onRefresh(): void;
  onFolder(collection: NuvioCollection, folder: CollectionFolder): void;
}) {
  return (
    <div className="collections-page">
      <div className="feature-title collections-title">
        <div>
          <span>NUVIO SYNC</span>
          <h1>Collections</h1>
          <p>Your custom collection folders follow the active profile.</p>
        </div>
        <button
          className="secondary-button"
          onClick={onRefresh}
          disabled={loading}
        >
          <Icon name="refresh" size={18} />
          Refresh
        </button>
      </div>
      {error ? (
        <div className="inline-error">{error}</div>
      ) : loading ? (
        <div className="library-loading">
          <i className="loading-spinner" /> Syncing collections…
        </div>
      ) : collections.length === 0 ? (
        <div className="empty-feature">
          <strong>No synced collections</strong>
          <span>Create a collection in Nuvio and refresh this profile.</span>
        </div>
      ) : (
        <CollectionRows collections={collections} onFolder={onFolder} />
      )}
    </div>
  );
}

export function CollectionFolderPage({
  collection,
  folder,
  progress,
  onBack,
  onSelect,
  onSeeAll,
}: {
  collection: NuvioCollection;
  folder: CollectionFolder;
  progress: ProgressSnapshot;
  onBack(): void;
  onSelect(item: ContentMeta): void;
  onSeeAll(section: CatalogSection): void;
}) {
  const [payload, setPayload] = useState<HomePayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState("all");
  const sources = useMemo(() => addonSources(folder), [folder]);
  useEffect(() => {
    setLoading(true);
    setError(null);
    setPayload(null);
    setActiveTab("all");
    invoke<HomePayload>("content.collectionFolder", { sources })
      .then(setPayload)
      .catch((reason: Error) => setError(reason.message))
      .finally(() => setLoading(false));
  }, [folder.id]);
  const allItems = useMemo(() => {
    const unique = new Map<string, ContentMeta>();
    for (const section of payload?.sections ?? [])
      for (const item of section.items)
        if (!unique.has(`${item.contentType}:${item.id}`))
          unique.set(`${item.contentType}:${item.id}`, item);
    return [...unique.values()];
  }, [payload]);
  const sections = payload?.sections ?? [];
  const showAll = collection.showAllTab && sections.length > 1;
  const selectedSection =
    sections.find((section) => section.key === activeTab) ?? sections[0];
  const selectedItems =
    activeTab === "all" && showAll ? allItems : (selectedSection?.items ?? []);
  const rowsMode = collection.viewMode.toUpperCase() !== "TABBED_GRID";
  const backdrop = folder.heroBackdropUrl || collection.backdropImageUrl;
  return (
    <div className="collection-folder-page">
      <header
        className="collection-folder-hero"
        style={
          backdrop
            ? {
                backgroundImage: `linear-gradient(0deg,#1b1c1e 0%,transparent 70%),linear-gradient(90deg,#111b 0%,transparent 70%),url("${backdrop.replaceAll('"', "%22")}")`,
              }
            : undefined
        }
      >
        <button
          className="round-back-button"
          onClick={onBack}
          aria-label="Back"
        >
          <Icon name="back" size={25} />
        </button>
        <div>
          {folder.titleLogoUrl ? (
            <img src={folder.titleLogoUrl} alt={folder.title} />
          ) : (
            <h1>{folder.title}</h1>
          )}
          <p>
            {collection.title} · {allItems.length} titles · {sections.length}{" "}
            catalog{sections.length === 1 ? "" : "s"}
          </p>
        </div>
      </header>
      {error && (
        <div className="inline-error collection-folder-error">{error}</div>
      )}
      {payload?.errors?.length ? (
        <div className="inline-error collection-folder-error">
          {payload.errors.length} collection source
          {payload.errors.length === 1 ? "" : "s"} could not be loaded.
        </div>
      ) : null}
      {loading ? (
        <div className="library-loading collection-folder-loading">
          <i className="loading-spinner" /> Loading collection sources…
        </div>
      ) : sources.length === 0 ? (
        <div className="empty-feature">
          <strong>This source is not available in the POC yet</strong>
          <span>
            Addon-backed folders work now; TMDB and Trakt collection sources
            will be added separately.
          </span>
        </div>
      ) : rowsMode ? (
        <div className="collection-catalog-rows">
          {sections.map((section) => (
            <MediaRow
              key={section.key}
              section={section}
              progress={progress}
              onSelect={onSelect}
              onSeeAll={onSeeAll}
            />
          ))}
        </div>
      ) : (
        <>
          <nav
            className="collection-source-tabs"
            aria-label="Collection catalogs"
          >
            {showAll && (
              <button
                className={activeTab === "all" ? "active" : ""}
                onClick={() => setActiveTab("all")}
              >
                All
              </button>
            )}
            {sections.map((section) => (
              <button
                className={
                  activeTab === section.key ||
                  (!showAll && selectedSection?.key === section.key)
                    ? "active"
                    : ""
                }
                key={section.key}
                onClick={() => setActiveTab(section.key)}
              >
                {section.title}
                <small>{section.subtitle}</small>
              </button>
            ))}
          </nav>
          {selectedSection && activeTab !== "all" && (
            <div className="collection-tab-actions">
              <button
                className="text-button"
                onClick={() => onSeeAll(selectedSection)}
              >
                View all
              </button>
            </div>
          )}
          <div className="catalog-grid collection-items-grid">
            {selectedItems.map((item) => (
              <button
                key={`${item.sourceManifestUrl}:${item.contentType}:${item.id}`}
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
        </>
      )}
    </div>
  );
}

function addonSources(folder: CollectionFolder): CollectionCatalogSource[] {
  if (folder.sources.length > 0)
    return folder.sources
      .filter(
        (source) =>
          !source.provider || source.provider.toLowerCase() === "addon",
      )
      .flatMap((source) =>
        source.addonId && source.type && source.catalogId
          ? [
              {
                addonId: source.addonId,
                type: source.type,
                catalogId: source.catalogId,
                genre: source.genre,
              },
            ]
          : [],
      );
  return folder.catalogSources;
}
function folderImage(folder: CollectionFolder) {
  return folder.focusGifEnabled
    ? folder.focusGifUrl || folder.coverImageUrl
    : folder.coverImageUrl;
}
function normalizedShape(shape: string) {
  return shape.toLowerCase() === "landscape" || shape.toLowerCase() === "wide"
    ? "landscape"
    : shape.toLowerCase() === "square"
      ? "square"
      : "poster";
}

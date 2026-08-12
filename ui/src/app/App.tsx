import { FormEvent, useEffect, useState } from "react";
import { invoke, onNativeEvent } from "../bridge/nativeBridge";
import type {
  AccountPayload,
  AddonRow,
  AuthSnapshot,
  AvatarCatalogItem,
  BootstrapResult,
  CatalogSection,
  CollectionFolder,
  ContentMeta,
  HomePayload,
  NuvioCollection,
  NuvioProfile,
  ProgressSnapshot,
  SettingsSnapshot,
  StreamSource,
} from "../bridge/types";
import { AddonsPage } from "../components/AddonsPage";
import { AuthGate } from "../components/AuthGate";
import { CatalogPage } from "../components/CatalogPage";
import { DetailsPage } from "../components/DetailsOverlay";
import type { PlayContext } from "../components/DetailsOverlay";
import { Icon } from "../components/Icon";
import { MediaRow } from "../components/MediaRow";
import { LibraryPage } from "../components/LibraryPage";
import { SettingsPage } from "../components/SettingsPage";
import { ProfileMenu } from "../components/ProfileMenu";
import { buildContinueWatching } from "../components/WatchStatus";
import { ContinueWatchingRow } from "../components/ContinueWatchingRow";
import {
  CollectionFolderPage,
  CollectionRows,
} from "../components/CollectionsPage";
import {
  onLibraryChanged,
  showTitleContextMenu,
  TitleContextMenu,
} from "../components/TitleContextMenu";
import { PlayerPage } from "../components/PlayerPage";
import type { ActivePlayback } from "../components/PlayerPage";

const navItems = [
  ["Home", "home"],
  ["Discover", "discover"],
  ["Library", "library"],
  ["Addons", "addons"],
  ["Settings", "settings"],
] as const;
const emptyAuth: AuthSnapshot = {
  status: "unauthenticated",
  backendConfigured: false,
  isAnonymous: false,
};
const emptyProgress: ProgressSnapshot = { entries: [], watchedItems: [] };

export function App() {
  const [activeNav, setActiveNav] = useState("Home");
  const [bootstrap, setBootstrap] = useState<BootstrapResult | null>(null);
  const [auth, setAuth] = useState<AuthSnapshot>(emptyAuth);
  const [profiles, setProfiles] = useState<NuvioProfile[]>([]);
  const [activeProfileIndex, setActiveProfileIndex] = useState(1);
  const [addons, setAddons] = useState<AddonRow[]>([]);
  const [bridgeStatus, setBridgeStatus] = useState("Connecting to Rust…");
  const [, setPlayerStatus] = useState("Player boundary ready");
  const [accountBusy, setAccountBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [home, setHome] = useState<HomePayload | null>(null);
  const [contentBusy, setContentBusy] = useState(false);
  const [contentError, setContentError] = useState<string | null>(null);
  const [selected, setSelected] = useState<ContentMeta | null>(null);
  const [query, setQuery] = useState("");
  const [searchResult, setSearchResult] = useState<HomePayload | null>(null);
  const [contentRevision, setContentRevision] = useState(0);
  const [catalogView, setCatalogView] = useState<CatalogSection | null>(null);
  const [catalogReturnNav, setCatalogReturnNav] = useState("Home");
  const [detailsReturnNav, setDetailsReturnNav] = useState("Home");
  const [libraryRevision, setLibraryRevision] = useState(0);
  const [amoled, setAmoled] = useState(false);
  const [avatars, setAvatars] = useState<AvatarCatalogItem[]>([]);
  const [progress, setProgress] = useState<ProgressSnapshot>(emptyProgress);
  const [progressLibrary, setProgressLibrary] = useState<ContentMeta[]>([]);
  const [progressMetadata, setProgressMetadata] = useState<ContentMeta[]>([]);
  const [progressRevision, setProgressRevision] = useState(0);
  const [collections, setCollections] = useState<NuvioCollection[]>([]);
  const [collectionsBusy, setCollectionsBusy] = useState(false);
  const [collectionsError, setCollectionsError] = useState<string | null>(null);
  const [collectionFolder, setCollectionFolder] = useState<{
    collection: NuvioCollection;
    folder: CollectionFolder;
  } | null>(null);
  const [collectionReturnNav, setCollectionReturnNav] = useState("Home");
  const [activePlayback, setActivePlayback] = useState<ActivePlayback | null>(
    null,
  );

  const addonSignature = addons
    .map((addon) => `${addon.enabled}:${addon.url}`)
    .join("|");

  useEffect(() => {
    invoke<BootstrapResult>("app.bootstrap")
      .then((result) => {
        setBootstrap(result);
        setAuth(result.auth);
        setProfiles(result.profiles);
        setActiveProfileIndex(result.activeProfileIndex);
        setAddons(result.addons);
        setBridgeStatus(`Rust ↔ JS · protocol v${result.protocolVersion}`);
      })
      .catch((error: Error) => setBridgeStatus(error.message));
    return onNativeEvent<{ state: string; detail: string }>(
      "player.stateChanged",
      (event) => setPlayerStatus(`${event.state} · ${event.detail}`),
    );
  }, []);

  useEffect(() => {
    if (auth.status !== "authenticated") return;
    setHome(null);
    setContentError(null);
    setContentBusy(true);
    invoke<HomePayload>("content.home")
      .then((payload) => {
        setHome(payload);
        if (payload.errors.length)
          setNotice(
            `${payload.errors.length} addon request${payload.errors.length === 1 ? "" : "s"} could not be loaded.`,
          );
      })
      .catch((error: Error) => setContentError(error.message))
      .finally(() => setContentBusy(false));
  }, [auth.status, activeProfileIndex, addonSignature, contentRevision]);

  useEffect(() => {
    if (auth.status === "authenticated")
      invoke<SettingsSnapshot>("settings.load")
        .then((settings) => setAmoled(settings.amoledEnabled))
        .catch(() => undefined);
  }, [auth.status, activeProfileIndex]);

  useEffect(() => {
    if (auth.status !== "authenticated") {
      setProgress(emptyProgress);
      setProgressLibrary([]);
      setProgressMetadata([]);
      return;
    }
    Promise.all([
      invoke<ProgressSnapshot>("progress.snapshot"),
      invoke<{ items: ContentMeta[] }>("library.list").catch(() => ({
        items: [],
      })),
    ])
      .then(async ([snapshot, library]) => {
        setProgress(snapshot);
        setProgressLibrary(library.items);
        const recentIdentities = [
          ...snapshot.entries.map((entry) => ({
            id: entry.contentId,
            type: entry.contentType,
            at: entry.lastWatched,
          })),
          ...snapshot.watchedItems.map((item) => ({
            id: item.contentId,
            type: item.contentType,
            at: item.watchedAt,
          })),
        ]
          .filter((item) => item.id && item.type)
          .sort((left, right) => right.at - left.at);
        const identities = new Map<string, string>();
        for (const item of recentIdentities)
          if (!identities.has(item.id)) identities.set(item.id, item.type);
        const resolved = await Promise.all(
          [...identities]
            .slice(0, 20)
            .map(([id, type]) =>
              invoke<ContentMeta>("content.resolveMeta", { id, type }).catch(
                () => null,
              ),
            ),
        );
        setProgressMetadata(
          resolved.filter((item): item is ContentMeta => !!item),
        );
      })
      .catch(() => {
        setProgress(emptyProgress);
        setProgressLibrary([]);
        setProgressMetadata([]);
      });
  }, [
    auth.status,
    activeProfileIndex,
    libraryRevision,
    contentRevision,
    progressRevision,
  ]);

  useEffect(() => {
    const refreshProgress = () =>
      setProgressRevision((revision) => revision + 1);
    window.addEventListener("focus", refreshProgress);
    return () => window.removeEventListener("focus", refreshProgress);
  }, []);

  useEffect(
    () => onLibraryChanged(() => setLibraryRevision((revision) => revision + 1)),
    [],
  );

  useEffect(() => {
    if (auth.status === "authenticated" && !auth.isAnonymous)
      invoke<{ avatars: AvatarCatalogItem[] }>("profiles.avatars")
        .then((result) => setAvatars(result.avatars))
        .catch(() => setAvatars([]));
  }, [auth.status, auth.isAnonymous]);

  useEffect(() => {
    if (auth.status === "authenticated") refreshCollections();
    else setCollections([]);
  }, [auth.status, activeProfileIndex]);

  async function refreshCollections() {
    setCollectionsBusy(true);
    setCollectionsError(null);
    try {
      setCollections(
        (await invoke<{ collections: NuvioCollection[] }>("collections.list"))
          .collections,
      );
    } catch (error) {
      setCollectionsError(
        error instanceof Error ? error.message : "Could not sync collections",
      );
    } finally {
      setCollectionsBusy(false);
    }
  }

  async function reorderCollection(
    collectionId: string,
    folderId: string | undefined,
    direction: -1 | 1,
  ) {
    setCollectionsBusy(true);
    setCollectionsError(null);
    try {
      setCollections(
        (
          await invoke<{ collections: NuvioCollection[] }>(
            "collections.reorder",
            { collectionId, folderId, direction },
          )
        ).collections,
      );
    } catch (error) {
      setCollectionsError(
        error instanceof Error
          ? error.message
          : "Could not reorder collections",
      );
    } finally {
      setCollectionsBusy(false);
    }
  }

  function applyAccount(payload: AccountPayload) {
    setAuth(payload.auth);
    setProfiles(payload.profiles);
    setActiveProfileIndex(payload.activeProfileIndex);
    setAddons(payload.addons);
    setNotice(payload.warning ?? null);
    setContentRevision((revision) => revision + 1);
  }

  async function selectProfile(profileIndex: number) {
    setAccountBusy(true);
    try {
      applyAccount(
        await invoke<AccountPayload>("profiles.select", { profileIndex }),
      );
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Could not switch profile",
      );
    } finally {
      setAccountBusy(false);
    }
  }
  async function refreshAddons() {
    setAccountBusy(true);
    try {
      applyAccount(await invoke<AccountPayload>("addons.refresh"));
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Could not refresh addons",
      );
    } finally {
      setAccountBusy(false);
    }
  }
  async function signOut() {
    setAccountBusy(true);
    try {
      applyAccount(await invoke<AccountPayload>("auth.signOut"));
      setHome(null);
      setActiveNav("Home");
    } finally {
      setAccountBusy(false);
    }
  }
  async function createProfile(input: {
    name: string;
    avatarId?: string;
    avatarUrl?: string;
    usesPrimaryAddons: boolean;
  }) {
    setAccountBusy(true);
    try {
      applyAccount(await invoke<AccountPayload>("profiles.create", input));
    } finally {
      setAccountBusy(false);
    }
  }
  async function preparePlayback(stream: StreamSource, context: PlayContext) {
    setActivePlayback({ title: context.title, context });
    try {
      const result = await invoke<{ status: string }>("player.prepare", {
        mediaId: context.title,
        url: stream.url,
        externalUrl: stream.externalUrl,
        requestHeaders: stream.behaviorHints?.proxyHeaders?.request,
        startPositionMs: context.startPositionMs,
        progress: {
          contentId: context.contentId,
          contentType: context.contentType,
          videoId: context.videoId,
          season: context.season,
          episode: context.episode,
        },
      });
      setPlayerStatus(result.status);
    } catch (error) {
      setActivePlayback(null);
      setNotice(
        error instanceof Error ? error.message : "Could not start playback",
      );
    }
  }
  function leavePlayback() {
    setActivePlayback(null);
    setProgressRevision((revision) => revision + 1);
  }
  async function submitSearch(event: FormEvent) {
    event.preventDefault();
    const value = query.trim();
    if (!value) return;
    setActiveNav("Search");
    setContentBusy(true);
    setContentError(null);
    setSearchResult(null);
    try {
      setSearchResult(
        await invoke<HomePayload>("content.search", { query: value }),
      );
    } catch (error) {
      setContentError(error instanceof Error ? error.message : "Search failed");
    } finally {
      setContentBusy(false);
    }
  }
  function openCatalog(section: CatalogSection) {
    setCatalogReturnNav(activeNav === "Catalog" ? "Home" : activeNav);
    setCatalogView(section);
    setActiveNav("Catalog");
  }
  function leaveCatalog() {
    setCatalogView(null);
    setActiveNav(catalogReturnNav);
  }
  function openDetails(item: ContentMeta) {
    setDetailsReturnNav(activeNav === "Details" ? "Home" : activeNav);
    setSelected(item);
    setActiveNav("Details");
  }
  function leaveDetails() {
    setSelected(null);
    setActiveNav(detailsReturnNav);
  }
  function openCollectionFolder(
    collection: NuvioCollection,
    folder: CollectionFolder,
  ) {
    setCollectionReturnNav(
      activeNav === "CollectionFolder" ? "Home" : activeNav,
    );
    setCollectionFolder({ collection, folder });
    setActiveNav("CollectionFolder");
  }
  function leaveCollectionFolder() {
    setCollectionFolder(null);
    setActiveNav(collectionReturnNav);
  }

  if (!bootstrap)
    return (
      <div className="boot-screen">
        <img src="/nuvio-wordmark.png" alt="Nuvio" />
        <span>{bridgeStatus}</span>
      </div>
    );
  if (auth.status !== "authenticated")
    return (
      <AuthGate
        backendConfigured={auth.backendConfigured}
        onAuthenticated={applyAccount}
      />
    );
  if (activePlayback)
    return (
      <PlayerPage
        playback={activePlayback}
        amoled={amoled}
        onBack={leavePlayback}
      />
    );

  return (
    <div className={amoled ? "app-shell amoled" : "app-shell"}>
      <aside className="sidebar">
        <div className="logo-mark">
          <img className="wordmark" src="/nuvio-wordmark.png" alt="Nuvio" />
        </div>
        <nav aria-label="Primary navigation">
          {navItems.map(([label, icon]) => (
            <button
              key={label}
              aria-label={label}
              title={label}
              className={activeNav === label ? "nav-item active" : "nav-item"}
              onClick={() => setActiveNav(label)}
            >
              <Icon name={icon} size={25} />
            </button>
          ))}
        </nav>
      </aside>
      <main className="content">
        <header className="topbar">
          <div className="topbar-spacer" />
          <form className="search-button search-form" onSubmit={submitSearch}>
            <Icon name="search" />
            <input
              aria-label="Search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search movies and series…"
            />
            <kbd>Enter</kbd>
          </form>
          <ProfileMenu
            profiles={profiles}
            activeIndex={activeProfileIndex}
            avatars={avatars}
            busy={accountBusy}
            canCreate={!auth.isAnonymous}
            onSelect={selectProfile}
            onCreate={createProfile}
            onSignOut={signOut}
          />
        </header>
        <div
          className={
            activeNav === "Details"
              ? "scroll-content details-route"
              : "scroll-content"
          }
        >
          {notice && (
            <div className="notice-banner">
              <span>{notice}</span>
              <button onClick={() => setNotice(null)}>Dismiss</button>
            </div>
          )}
          {activeNav === "Addons" ? (
            <AddonsPage
              addons={addons}
              loading={accountBusy}
              onAccount={applyAccount}
              onRefresh={refreshAddons}
            />
          ) : activeNav === "Settings" ? (
            <SettingsPage
              profileIndex={activeProfileIndex}
              collections={collections}
              collectionsLoading={collectionsBusy}
              collectionsError={collectionsError}
              onRefreshCollections={refreshCollections}
              onReorderCollection={reorderCollection}
              onOpenFolder={openCollectionFolder}
              onThemeChange={setAmoled}
            />
          ) : activeNav === "Library" ? (
            <LibraryPage
              profileIndex={activeProfileIndex}
              revision={libraryRevision}
              progress={progress}
              onSelect={openDetails}
            />
          ) : activeNav === "CollectionFolder" && collectionFolder ? (
            <CollectionFolderPage
              {...collectionFolder}
              progress={progress}
              onBack={leaveCollectionFolder}
              onSelect={openDetails}
              onSeeAll={openCatalog}
            />
          ) : activeNav === "Details" && selected ? (
            <DetailsPage
              seed={selected}
              progress={progress}
              onBack={leaveDetails}
              onPlay={preparePlayback}
              onLibraryChange={() => setLibraryRevision((value) => value + 1)}
            />
          ) : activeNav === "Home" ? (
            <ContentPage
              payload={home}
              collections={collections}
              progress={progress}
              progressLibrary={[...progressLibrary, ...progressMetadata]}
              busy={contentBusy}
              error={contentError}
              addons={addons}
              onCollectionFolder={openCollectionFolder}
              onSelect={openDetails}
              onSeeAll={openCatalog}
            />
          ) : activeNav === "Search" ? (
            <SearchPage
              query={query}
              payload={searchResult}
              progress={progress}
              busy={contentBusy}
              error={contentError}
              onSelect={openDetails}
              onSeeAll={openCatalog}
            />
          ) : activeNav === "Catalog" && catalogView ? (
            <CatalogPage
              source={catalogView}
              progress={progress}
              onBack={leaveCatalog}
              onSelect={openDetails}
            />
          ) : (
            <div className="feature-page">
              <div className="feature-title">
                <div>
                  <span>INCREMENTAL MIGRATION</span>
                  <h1>{activeNav}</h1>
                  <p>This route is ready for its Rust service.</p>
                </div>
              </div>
              <div className="empty-feature">
                <strong>{activeNav} is not migrated yet</strong>
                <span>
                  Addon catalogs, search, metadata and stream lookup are live
                  now.
                </span>
              </div>
            </div>
          )}
        </div>
        <TitleContextMenu onSelect={openDetails} />
      </main>
    </div>
  );
}

function ContentPage({
  payload,
  collections,
  progress,
  progressLibrary,
  busy,
  error,
  addons,
  onCollectionFolder,
  onSelect,
  onSeeAll,
}: {
  payload: HomePayload | null;
  collections: NuvioCollection[];
  progress: ProgressSnapshot;
  progressLibrary: ContentMeta[];
  busy: boolean;
  error: string | null;
  addons: AddonRow[];
  onCollectionFolder(
    collection: NuvioCollection,
    folder: CollectionFolder,
  ): void;
  onSelect(item: ContentMeta): void;
  onSeeAll(section: CatalogSection): void;
}) {
  if (busy) return <LoadingRows label="Loading your addon catalogs…" />;
  if (error) return <Empty title="Catalog loading failed" detail={error} />;
  if (!addons.some((addon) => addon.enabled))
    return (
      <Empty
        title="No enabled addons"
        detail="Install or enable a Stremio-compatible addon in Nuvio, then refresh this profile."
      />
    );
  if (!payload?.sections.length)
    return (
      <Empty
        title="No home catalogs returned"
        detail="Your addons loaded, but none exposed a non-required home catalog."
      />
    );
  const heroItems = payload.heroItems?.length
    ? payload.heroItems
    : payload.hero
      ? [payload.hero]
      : [];
  const catalogItems = payload.sections.flatMap((section) => section.items);
  const continuing = buildContinueWatching(progress, [
    ...catalogItems,
    ...progressLibrary,
  ]);
  const orderedCollections = [...collections].sort(
    (left, right) => Number(right.pinToTop) - Number(left.pinToTop),
  );
  return (
    <>
      <HomeHero items={heroItems} onSelect={onSelect} />
      {continuing.length > 0 && (
        <ContinueWatchingRow cards={continuing} onSelect={onSelect} />
      )}
      <CollectionRows
        collections={orderedCollections}
        onFolder={onCollectionFolder}
      />
      {payload.sections.map((section) => (
        <MediaRow
          key={section.key}
          section={section}
          progress={progress}
          onSelect={onSelect}
          onSeeAll={onSeeAll}
        />
      ))}
    </>
  );
}

function HomeHero({
  items,
  onSelect,
}: {
  items: ContentMeta[];
  onSelect(item: ContentMeta): void;
}) {
  const [activeIndex, setActiveIndex] = useState(0);
  const [logoFailed, setLogoFailed] = useState(false);
  const active = items[activeIndex % Math.max(items.length, 1)];

  useEffect(() => {
    if (activeIndex >= items.length) setActiveIndex(0);
  }, [activeIndex, items.length]);

  useEffect(() => {
    setLogoFailed(false);
  }, [active?.id, active?.logo]);

  useEffect(() => {
    if (items.length < 2) return;
    const timer = window.setInterval(
      () => setActiveIndex((index) => (index + 1) % items.length),
      9000,
    );
    return () => window.clearInterval(timer);
  }, [items.length]);

  if (!active) return null;
  const artwork = active.background || active.banner || active.poster;
  const genre =
    active.genres.slice(0, 3).join(" • ") ||
    (active.contentType === "series" ? "Series" : "Movie");
  return (
    <section
      key={`${active.contentType}:${active.id}`}
      className="home-hero"
      onContextMenu={(event) => showTitleContextMenu(event, active)}
      style={
        artwork
          ? { backgroundImage: `url("${artwork.replace(/"/g, '\\"')}")` }
          : undefined
      }
    >
      <button
        className="home-hero-expand"
        aria-label={`View details for ${active.name}`}
        onClick={() => onSelect(active)}
      >
        <Icon name="fullscreen" size={25} />
      </button>
      <div className="home-hero-copy">
        {active.logo && !logoFailed ? (
          <img
            className="home-hero-logo"
            src={active.logo}
            alt={active.name}
            onError={() => setLogoFailed(true)}
          />
        ) : (
          <h1>{active.name}</h1>
        )}
        <strong className="home-hero-genre">{genre}</strong>
        <p>{active.description || "Open details to learn more."}</p>
        <button className="home-hero-details" onClick={() => onSelect(active)}>
          View Details
        </button>
      </div>
      {items.length > 1 && (
        <div className="home-hero-pages" aria-label="Featured titles">
          {items.map((item, index) => (
            <button
              key={`${item.contentType}:${item.id}:${index}`}
              className={index === activeIndex ? "active" : ""}
              aria-label={`Show ${item.name}`}
              onClick={() => setActiveIndex(index)}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function SearchPage({
  query,
  payload,
  progress,
  busy,
  error,
  onSelect,
  onSeeAll,
}: {
  query: string;
  payload: HomePayload | null;
  progress: ProgressSnapshot;
  busy: boolean;
  error: string | null;
  onSelect(item: ContentMeta): void;
  onSeeAll(section: CatalogSection): void;
}) {
  return (
    <div className="search-page">
      <div className="feature-title">
        <div>
          <span>ADDON SEARCH</span>
          <h1>Results for “{query}”</h1>
          <p>Queried every compatible search catalog concurrently.</p>
        </div>
      </div>
      {busy ? (
        <LoadingRows label="Searching addons…" />
      ) : error ? (
        <Empty title="Search failed" detail={error} />
      ) : !payload?.sections.length ? (
        <Empty
          title="No matches"
          detail="No installed addon returned a result for this search."
        />
      ) : (
        payload.sections.map((section) => (
          <MediaRow
            key={section.key}
            section={section}
            progress={progress}
            onSelect={onSelect}
            onSeeAll={onSeeAll}
          />
        ))
      )}
    </div>
  );
}

function LoadingRows({ label }: { label: string }) {
  return (
    <div className="loading-page">
      <span className="loading-spinner" />
      <strong>{label}</strong>
      <div className="skeleton-hero" />
      <div className="skeleton-row">
        {Array.from({ length: 6 }, (_, index) => (
          <i key={index} />
        ))}
      </div>
    </div>
  );
}
function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="feature-page">
      <div className="empty-feature">
        <strong>{title}</strong>
        <span>{detail}</span>
      </div>
    </div>
  );
}

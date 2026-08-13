import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { ContentMeta, ExternalRating, LibraryItem, MetaTrailer, ProgressSnapshot, ResumePoint, SettingsSnapshot, StreamSource, Video } from "../bridge/types";
import { Icon } from "./Icon";
import { EpisodeBadge, WatchStatus, latestResumeFor, resumeForVideo, watchStateForContent, watchStateForEpisode } from "./WatchStatus";
import { EpisodeContextMenu, showEpisodeContextMenu } from "./EpisodeMenu";
import { getWatchedOverride, reconcileWatchedOverrides, useWatchedOverrides, watchedKey } from "../data/watchedOverrides";
import { isInLibrary, setLibraryMembership } from "../data/libraryCache";
import { cachedStreamToSource, contentKey, getValidStreamLink, saveStreamLink } from "../data/streamLinkCache";

/**
 * Enriched metadata survives navigation, so returning to a title does not
 * re-fetch every addon. Watched state is not cached — it comes from the live
 * progress snapshot each render.
 */
const detailsCache = new Map<string, ContentMeta>();

export function cachedDetailsKey(item: Pick<ContentMeta, "id" | "contentType">) {
  return `${item.contentType}:${item.id}`;
}

export type PlayContext = { title: string; startPositionMs: number; contentId: string; contentType: string; videoId: string; season?: number; episode?: number; videos?: Video[]; showName?: string; backdrop?: string; logo?: string };

export function DetailsPage({ seed, progress, settings, autoOpenSources, onBack, onPlay, onLibraryChange, onProgressChanged }: { seed: ContentMeta; progress: ProgressSnapshot; settings?: SettingsSnapshot | null; autoOpenSources?: boolean; onProgressChanged?(): void; onBack(): void; onPlay(stream: StreamSource, context: PlayContext): void; onLibraryChange?(): void; onProgressChanged?(): void }) {
  const [details, setDetails] = useState(seed);
  const [baseReady, setBaseReady] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [season, setSeason] = useState<number | null>(null);
  const [focusedVideo, setFocusedVideo] = useState<Video | null>(null);
  const [resume, setResume] = useState<ResumePoint | null>(null);
  const [saved, setSaved] = useState(false);
  const [libraryBusy, setLibraryBusy] = useState(false);
  const [episodeQuery, setEpisodeQuery] = useState("");
  const [sourceTarget, setSourceTarget] = useState<{ id: string; title: string; startPositionMs: number; season?: number; episode?: number } | null>(null);
  const [showDescription, setShowDescription] = useState(false);
  const [showTrailers, setShowTrailers] = useState(false);
  const [pendingWatched, setPendingWatched] = useState<boolean | null>(null);
  const mainColumnRef = useRef<HTMLElement | null>(null);
  const episodeListRef = useRef<HTMLDivElement | null>(null);
  const isSeries = details.contentType === "series" || details.contentType === "tv";
  const seasons = useMemo(() => orderSeasons([...new Set((details.videos ?? []).map((video) => video.season ?? 0))]), [details.videos]);
  const visibleEpisodes = useMemo(() => {
    const query = episodeQuery.trim().toLowerCase();
    return details.videos.filter((video) => {
      if ((video.season ?? 0) !== (season ?? seasons[0])) return false;
      if (!query) return true;
      return `${video.episode ?? ""} ${video.title} ${video.overview ?? ""}`.toLowerCase().includes(query);
    });
  }, [details.videos, season, seasons, episodeQuery]);
  const watchVideo = focusedVideo ?? details.videos.find((video) => video.id === resume?.videoId) ?? details.videos.find((video) => video.id === details.defaultVideoId) ?? details.videos.find((video) => video.available !== false) ?? null;

  const seedKey = cachedDetailsKey(seed);
  useEffect(() => {
    setSourceTarget(null); setEpisodeQuery(""); setError(null); setPendingWatched(null);

    function applyMeta(result: ContentMeta, usableResume: ResumePoint | null) {
      setDetails(result);
      setResume(usableResume);
      const requested = result.videos.find((video) => video.id === seed.selectedVideoId);
      const resumed = result.videos.find((video) => video.id === usableResume?.videoId);
      const defaultVideo = result.videos.find((video) => video.id === result.defaultVideoId);
      const firstRegularEpisode = firstEpisode(result.videos);
      const initial = requested ?? resumed ?? (defaultVideo?.season === 0 ? null : defaultVideo) ?? firstRegularEpisode ?? result.videos[0];
      setFocusedVideo(initial ?? null);
      setSeason(initial?.season ?? orderSeasons([...new Set(result.videos.map((video) => video.season ?? 0))])[0] ?? null);
      setSaved(isInLibrary(result));
      setBaseReady(true);
      setLoading(false);
    }

    // Cache hit: render immediately. Only the resume point is re-derived, and
    // that comes from the progress snapshot already in memory.
    const cached = detailsCache.get(seedKey);
    if (cached) {
      applyMeta(cached, latestResumeFor(progress, cached.id));
      return;
    }

    setBaseReady(false); setLoading(true); setFocusedVideo(null);
    Promise.all([
      invoke<ContentMeta>("content.details", { type: seed.contentType, id: seed.id }),
      invoke<{ resume?: ResumePoint }>("progress.resume", { id: seed.id }).catch(() => ({ resume: undefined })),
      invoke<{ items: LibraryItem[] }>("library.list").catch(() => ({ items: [] })),
    ]).then(([result, resumeResult]) => {
      const resumePercent = resumeResult.resume && resumeResult.resume.durationMs > 0 ? resumeResult.resume.positionMs / resumeResult.resume.durationMs : 0;
      const usableResume = resumeResult.resume && resumePercent < .9 ? resumeResult.resume : null;
      detailsCache.set(seedKey, result);
      applyMeta(result, usableResume);
      invoke<ContentMeta>("content.enrichMeta", { item: result }).then((enriched) => {
        detailsCache.set(cachedDetailsKey(enriched), enriched);
        setDetails((current) => current.id === enriched.id ? enriched : current);
        setFocusedVideo((current) => current ? enriched.videos.find((video) => video.id === current.id) ?? current : firstEpisode(enriched.videos));
      }).catch(() => undefined);
    }).catch((reason: Error) => { setError(reason.message); setLoading(false); });
  }, [seedKey, seed.selectedVideoId]);

  useEffect(() => {
    if (!baseReady) return;
    mainColumnRef.current?.scrollTo({ top: 0, left: 0 });
    episodeListRef.current?.scrollTo({ top: 0, left: 0 });
  }, [baseReady, details.id]);

  // Opened from Continue Watching: go straight to sources for the resumed video.
  const autoOpened = useRef(false);
  useEffect(() => {
    if (!autoOpenSources || !baseReady || autoOpened.current) return;
    autoOpened.current = true;
    openSources(focusedVideo);
  }, [autoOpenSources, baseReady, focusedVideo]);

  function openSources(video?: Video | null) {
    const target = isSeries ? (video ?? watchVideo) : null;
    if (isSeries && !target) { setError("This series did not provide any playable episodes."); return; }
    setFocusedVideo(target);
    const playableId = target?.id || details.id;
    // Look the position up per-video rather than trusting the title-level
    // resume, which only ever points at the most recently watched episode.
    const videoResume = resumeForVideo(progress, details.id, playableId, target?.season, target?.episode);
    const startPositionMs = videoResume?.positionMs ?? (resume && playableId === resume.videoId ? resume.positionMs : 0);
    const title = target?.title || details.name;

    // Reuse last stream: play the previous link straight away rather than
    // re-scraping every addon, matching Nuvio's StreamLinkCacheRepository.
    if (settings?.reuseLastStream) {
      const cached = getValidStreamLink(
        contentKey(details.contentType, playableId, details.id, target?.season, target?.episode),
        (settings.reuseLastStreamHours ?? 24) * 3_600_000,
      );
      if (cached) {
        onPlay(cachedStreamToSource(cached), {
          title, startPositionMs, contentId: details.id, contentType: details.contentType,
          videoId: playableId, season: target?.season, episode: target?.episode,
          videos: details.videos, showName: details.name,
          backdrop: details.background || details.banner, logo: details.logo,
        });
        return;
      }
    }

    setSourceTarget({ id: playableId, title, startPositionMs, season: target?.season, episode: target?.episode });
  }

  async function toggleLibrary() {
    setLibraryBusy(true); setError(null);
    try {
      if (saved) await invoke("library.remove", { type: details.contentType, id: details.id });
      else await invoke("library.add", { item: details });
      setLibraryMembership(details, !saved); setSaved(!saved); onLibraryChange?.();
    } catch (reason) { setError(reason instanceof Error ? reason.message : "Library update failed"); }
    finally { setLibraryBusy(false); }
  }

  // Optimistic: flip immediately, then let the refreshed snapshot take over.
  // The override is held (not cleared on success) until the server state agrees,
  // otherwise the icon would snap back for the frame between the write landing
  // and the progress snapshot arriving.
  const serverWatched = !isSeries && watchStateForContent(details, progress)?.watched === true;
  const movieWatched = pendingWatched ?? serverWatched;

  useEffect(() => {
    if (pendingWatched != null && serverWatched === pendingWatched) setPendingWatched(null);
  }, [pendingWatched, serverWatched]);

  // Release episode overrides the refreshed snapshot has caught up with.
  useWatchedOverrides();
  useEffect(() => {
    reconcileWatchedOverrides((key) => {
      const video = details.videos.find(
        (item) => watchedKey(details.id, item.season, item.episode) === key,
      );
      if (!video) return false;
      return watchStateForEpisode(details.id, video.season, video.episode, video.id, progress)?.watched === true;
    });
  }, [progress, details]);

  async function toggleMovieWatched() {
    const next = !movieWatched;
    setPendingWatched(next);
    setError(null);
    try {
      await invoke("progress.setWatched", {
        identity: {
          contentId: details.id,
          contentType: details.contentType,
          videoId: details.id,
        },
        title: details.name,
        watched: next,
      });
      onProgressChanged?.();
    } catch (reason) {
      // Roll back to whatever the server actually holds.
      setPendingWatched(null);
      setError(reason instanceof Error ? reason.message : "Could not update watched status");
    }
  }

  const description =
    details.description ||
    (loading ? "Loading metadata…" : "No description supplied by this addon.");
  // The button carries what the old "Resume" line underneath used to say, so
  // the episode and resume point are visible without a second row of text.
  const playLabel = [
    "Play",
    isSeries && watchVideo
      ? `S${watchVideo.season ?? 0} E${watchVideo.episode ?? 1}`
      : null,
    resume ? formatTime(resume.positionMs) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  const ratings = useMemo(() => {
    if (details.externalRatings.some((rating) => rating.source.toLowerCase() === "imdb") || !details.imdbRating) return details.externalRatings;
    const imdb = Number(details.imdbRating);
    return Number.isFinite(imdb) && imdb > 0 ? [{ source: "imdb", value: imdb }, ...details.externalRatings] : details.externalRatings;
  }, [details.externalRatings, details.imdbRating]);
  const backdrop = details.background || details.banner;
  const pageStyle = backdrop ? ({ "--detail-backdrop": `url("${backdrop.replaceAll('"', '%22')}")` } as CSSProperties) : undefined;
  if (!baseReady) return <div className="details-page details-metadata-loading">
    <button className="details-back round-back-button" aria-label="Back" title="Back" onClick={onBack}><Icon name="back" size={25} /></button>
    <div className="details-metadata-loading-copy">{loading ? <i className="loading-spinner details-loading-spinner" /> : <><strong>Metadata could not be loaded</strong>{error && <span>{error}</span>}</>}</div>
  </div>;
  return <div className={isSeries ? "details-page series-detail-page" : "details-page movie-detail-page"} style={pageStyle}>
    <main className="details-main-column" ref={mainColumnRef}>
    <button className="details-back round-back-button" aria-label="Back" title="Back" onClick={onBack}><Icon name="back" size={25} /></button>
    <section className="details-hero">
      <div className="details-hero-content"><div className="details-page-copy">{details.logo ? <MetadataLogo src={details.logo} name={details.name} /> : <h1>{details.name}</h1>}<div className="details-facts"><span>{details.releaseInfo || details.released?.slice(0, 4)}</span>{isSeries && seasons.filter((item) => item > 0).length > 0 && <span>{seasons.filter((item) => item > 0).length} Seasons</span>}{details.runtime && <span>{details.runtime}</span>}{details.ageRating && <span>{details.ageRating}</span>}{details.status && <span>{details.status}</span>}</div>{ratings.length > 0 && <DetailsRatings ratings={ratings} />}{details.genres?.length > 0 && <div className="genre-line">{details.genres.map((genre) => <span key={genre}>{genre}</span>)}</div>}<DetailsDescription text={description} onExpand={() => setShowDescription(true)} /><div className="details-actions"><button className="primary-button details-source-button" onClick={() => openSources()} disabled={loading}><Icon name="play" size={18} />{playLabel}</button><button className={saved ? "icon-pill active" : "icon-pill"} title={saved ? "Remove from library" : "Add to library"} onClick={toggleLibrary} disabled={libraryBusy}><Icon name={saved ? "check" : "plus"} size={19} /></button>{!isSeries && <button className={movieWatched ? "icon-pill active" : "icon-pill"} title={movieWatched ? "Watched \u2014 click to clear" : "Mark as watched"} onClick={toggleMovieWatched}><Icon name="eye" size={19} /></button>}{details.trailers.length > 0 && <button className="icon-pill" title={`Trailers & extras (${details.trailers.length})`} onClick={() => setShowTrailers(true)}><Icon name="video" size={19} /></button>}</div>{error && <div className="inline-error">{error}</div>}</div></div>
    </section>
    {(details.director.length > 0 || details.writer.length > 0 || details.language) && <section className="credits-strip">{details.director.length > 0 && <Credit label="Director" value={details.director.join(", ")} />}{details.writer.length > 0 && <Credit label="Writer" value={details.writer.join(", ")} />}{details.language && <Credit label="Language" value={details.language} />}</section>}
    {details.cast.length > 0 && <section className="people-section"><div className="section-title"><span>CAST</span><h2>Actors</h2></div><div className="people-row">{details.cast.map((person) => <article className="person-card" key={`${person.name}:${person.role ?? ""}`}>{person.photo ? <img src={person.photo} alt="" /> : <div className="person-placeholder">{person.name.slice(0, 1)}</div>}<strong>{person.name}</strong>{person.role && <span>{person.role}</span>}</article>)}</div></section>}
    </main>
    {isSeries && <section className="episodes-section"><div className="episodes-heading"><div><span>EPISODES</span><h2>{details.name}</h2></div><label className="season-select-wrap"><span>Season</span><select value={season ?? seasons[0] ?? 0} onChange={(event) => { const next = Number(event.target.value); setSeason(next); setEpisodeQuery(""); setFocusedVideo(details.videos.find((video) => (video.season ?? 0) === next) ?? null); episodeListRef.current?.scrollTo({ top: 0 }); }}>{seasons.map((item) => <option value={item} key={item}>{item === 0 ? "Specials" : `Season ${item}`}</option>)}</select></label></div><label className="episode-search"><Icon name="search" size={19} /><input value={episodeQuery} onChange={(event) => setEpisodeQuery(event.target.value)} placeholder="Search this season" /></label><div className="episode-list-heading"><strong>{season === 0 ? "Specials" : `Season ${season ?? seasons[0] ?? 1}`}</strong><span>{visibleEpisodes.length} episodes</span></div><div className="episode-grid" ref={episodeListRef}>{visibleEpisodes.map((video, index) => <button className={focusedVideo?.id === video.id ? "episode-card selected" : "episode-card"} key={video.id} onClick={() => openSources(video)} onContextMenu={(event) => showEpisodeContextMenu(event, { details, video, watched: getWatchedOverride(watchedKey(details.id, video.season, video.episode)) ?? (watchStateForEpisode(details.id, video.season, video.episode, video.id, progress)?.watched === true) })}><div className="episode-thumb">{video.thumbnail ? <img src={video.thumbnail} alt="" /> : <div className="episode-placeholder"><Icon name="play" /></div>}<WatchStatus state={watchStateForEpisode(details.id, video.season, video.episode, video.id, progress)} /><EpisodeBadge contentId={details.id} videoId={video.id} season={video.season} episode={video.episode} snapshot={progress} /></div><div><small>{video.episode ? `EPISODE ${video.episode}` : `EPISODE ${index + 1}`}{video.released ? ` · ${formatDate(video.released)}` : ""}</small><strong>{video.title || `Episode ${video.episode ?? index + 1}`}</strong><span>{video.overview || (video.available === false ? "Not available yet" : "Select to choose a source")}</span></div></button>)}</div></section>}
    <EpisodeContextMenu onChanged={() => onProgressChanged?.()} />
    {sourceTarget && <SourcePicker target={sourceTarget} contentId={details.id} contentType={details.contentType} onClose={() => setSourceTarget(null)} onPlay={(stream, context) => {
      // Remember the pick so "Reuse last stream" can skip the picker next time.
      saveStreamLink(contentKey(details.contentType, context.videoId, details.id, context.season, context.episode), stream);
      onPlay(stream, { ...context, videos: details.videos, showName: details.name, backdrop: details.background || details.banner, logo: details.logo });
    }} />}
    {showDescription && <DetailsModal title={details.name} onClose={() => setShowDescription(false)}><p className="details-modal-description">{description}</p></DetailsModal>}
    {showTrailers && <TrailerModal trailers={details.trailers} onClose={() => setShowTrailers(false)} />}
  </div>;
}

/** Clamps a long synopsis and offers the full text in a modal. */
function DetailsDescription({ text, onExpand }: { text: string; onExpand(): void }) {
  const paragraph = useRef<HTMLParagraphElement>(null);
  const [clipped, setClipped] = useState(false);

  // Measured rather than guessed from character count: the same string clips or
  // not depending on window width and the chosen font size.
  useEffect(() => {
    const element = paragraph.current;
    if (!element) return;
    const measure = () =>
      setClipped(element.scrollHeight - element.clientHeight > 4);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [text]);

  return (
    <div className="details-description">
      <p ref={paragraph} className="clamped">{text}</p>
      {clipped && (
        <button className="read-more" onClick={onExpand}>
          Read more
        </button>
      )}
    </div>
  );
}

/** Plays trailers in an embedded YouTube frame rather than handing them off to
 *  the system browser. Falls back to opening externally for any non-YouTube
 *  site, which cannot be embedded from a thumbnail key alone. */
function TrailerModal({ trailers, onClose }: { trailers: MetaTrailer[]; onClose(): void }) {
  const [active, setActive] = useState<MetaTrailer | null>(null);
  const embeddable = (trailer: MetaTrailer) =>
    !trailer.site || trailer.site.toLowerCase() === "youtube";

  function open(trailer: MetaTrailer) {
    if (embeddable(trailer)) {
      setActive(trailer);
      return;
    }
    void invoke("system.openExternal", {
      url: `https://www.youtube.com/watch?v=${encodeURIComponent(trailer.key)}`,
    });
  }

  return (
    <DetailsModal
      title={active ? active.name : "Trailers & extras"}
      onClose={onClose}
      onBack={active ? () => setActive(null) : undefined}
    >
      {active ? (
        <div className="trailer-player">
          <iframe
            src={`https://www.youtube-nocookie.com/embed/${encodeURIComponent(active.key)}?autoplay=1&rel=0&modestbranding=1`}
            title={active.name}
            allow="accelerometer; autoplay; encrypted-media; gyroscope; picture-in-picture"
            allowFullScreen
          />
          <button
            className="text-button"
            onClick={() =>
              invoke("system.openExternal", {
                url: `https://www.youtube.com/watch?v=${encodeURIComponent(active.key)}`,
              })
            }
          >
            Open on YouTube instead
          </button>
        </div>
      ) : (
        <div className="trailer-grid">
          {trailers.map((trailer) => (
            <button
              key={trailer.id || trailer.key}
              title={embeddable(trailer) ? "Play here" : "Open in your browser"}
              onClick={() => open(trailer)}
            >
              <span className="trailer-thumb">
                <img
                  src={`https://i.ytimg.com/vi/${encodeURIComponent(trailer.key)}/hqdefault.jpg`}
                  alt=""
                />
                <i>
                  <Icon name="play" size={18} />
                </i>
              </span>
              <strong>{trailer.name}</strong>
              <span className="trailer-meta">
                {trailer.site || "YouTube"}
                {trailer.trailerType ? ` · ${trailer.trailerType}` : ""}
              </span>
            </button>
          ))}
        </div>
      )}
    </DetailsModal>
  );
}

function DetailsModal({ title, onClose, onBack, children }: { title: string; onClose(): void; onBack?(): void; children: React.ReactNode }) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div
      className="details-modal-scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section className="details-modal">
        <header>
          {onBack && (
            <button className="modal-icon-button" aria-label="Back" title="Back" onClick={onBack}>
              <Icon name="back" size={20} />
            </button>
          )}
          <h2>{title}</h2>
          <button className="modal-icon-button" aria-label="Close" title="Close" onClick={onClose}>
            <Icon name="close" size={20} />
          </button>
        </header>
        <div className="details-modal-body">{children}</div>
      </section>
    </div>
  );
}

function MetadataLogo({ src, name }: { src: string; name: string }) {
  const isTmdbArtwork = src.toLowerCase().includes("image.tmdb.org/");
  const [appearance, setAppearance] = useState<"checking" | "normal" | "dark" | "failed">(isTmdbArtwork ? "checking" : "normal");
  useEffect(() => {
    if (!isTmdbArtwork) { setAppearance("normal"); return; }
    setAppearance("checking");
    const probe = new Image();
    probe.crossOrigin = "anonymous";
    probe.onload = () => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = 80; canvas.height = 40;
        const context = canvas.getContext("2d", { willReadFrequently: true });
        if (!context) return;
        context.drawImage(probe, 0, 0, canvas.width, canvas.height);
        const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
        let red = 0, green = 0, blue = 0, count = 0;
        for (let index = 0; index < pixels.length; index += 4) {
          if (pixels[index + 3] < 48) continue;
          red += pixels[index]; green += pixels[index + 1]; blue += pixels[index + 2]; count++;
        }
        if (!count) return;
        const channels = [red / count, green / count, blue / count];
        const luminance = channels[0] * .2126 + channels[1] * .7152 + channels[2] * .0722;
        const chroma = Math.max(...channels) - Math.min(...channels);
        setAppearance(luminance < 125 && chroma < 70 ? "dark" : "normal");
      } catch { setAppearance("normal"); }
    };
    // The brightness probe is optional. TMDB's image can still display even
    // when canvas inspection is blocked by WebView2's CORS policy.
    probe.onerror = () => setAppearance("normal");
    probe.src = src;
    return () => { probe.onload = null; probe.onerror = null; };
  }, [src, isTmdbArtwork]);
  if (appearance === "failed") return <h1>{name}</h1>;
  return <img className={appearance === "dark" ? "details-logo dark-artwork" : "details-logo"} src={src} alt={name} onError={() => setAppearance("failed")} />;
}

function SourcePicker({ target, contentId, contentType, onClose, onPlay }: { target: { id: string; title: string; startPositionMs: number; season?: number; episode?: number }; contentId: string; contentType: string; onClose(): void; onPlay(stream: StreamSource, context: PlayContext): void }) {
  const [streams, setStreams] = useState<StreamSource[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("all");
  const addonGroups = useMemo(() => {
    const groups = new Map<string, { id: string; name: string; logo?: string; streams: StreamSource[] }>();
    for (const stream of streams ?? []) {
      const id = stream.addonId || stream.addonName || "unknown";
      const group = groups.get(id) ?? { id, name: stream.addonName || "Stremio addon", logo: stream.addonLogo, streams: [] };
      group.streams.push(stream); groups.set(id, group);
    }
    return [...groups.values()];
  }, [streams]);
  const visibleGroups = filter === "all" ? addonGroups : addonGroups.filter((group) => group.id === filter);
  async function fetchStreams() {
    setStreams(null); setError(null);
    try { setStreams((await invoke<{ streams: StreamSource[] }>("content.streams", { type: contentType, id: target.id })).streams); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "Stream lookup failed"); }
  }
  useEffect(() => { fetchStreams(); }, [target.id, contentType]);
  return <div className="source-picker-scrim" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><section className="source-picker nuvio-source-picker"><button className="source-picker-close icon-close-button" aria-label="Close sources" onClick={onClose}><Icon name="close" size={22} /></button><div className="source-picker-heading"><span>CHOOSE A SOURCE</span><h2>{target.title}</h2><p>{streams ? `${streams.length} source${streams.length === 1 ? "" : "s"} from ${addonGroups.length} addon${addonGroups.length === 1 ? "" : "s"}` : "Fetching streams from compatible addons…"}</p></div><div className="source-filters"><button className="source-refresh" aria-label="Refresh sources" title="Refresh sources" onClick={fetchStreams}><Icon name="refresh" size={20} /></button><button className={filter === "all" ? "active" : ""} onClick={() => setFilter("all")}>All</button>{addonGroups.map((group) => <button className={filter === group.id ? "active" : ""} key={group.id} onClick={() => setFilter(group.id)}>{group.logo && <img src={group.logo} alt="" />}{group.name}</button>)}</div>{error && <div className="inline-error">{error}</div>}{!streams && !error ? <div className="sources-loading"><i className="loading-spinner" /><strong>Checking stream addons</strong></div> : streams?.length === 0 ? <div className="source-empty">No compatible source was returned.</div> : <div className="source-groups">{visibleGroups.map((group) => <section className="source-group" key={group.id}><div className="source-group-heading">{group.logo && <img src={group.logo} alt="" />}<strong>{group.name}</strong><span>{group.streams.length}</span></div><div className="source-list">{group.streams.map((stream, index) => <StreamButton key={`${group.id}:${index}`} stream={stream} index={index} onClick={() => onPlay(stream, { title: target.title, startPositionMs: target.startPositionMs, contentId, contentType, videoId: target.id, season: target.season, episode: target.episode })} />)}</div></section>)}</div>}</section></div>;
}

function StreamButton({ stream, index, onClick }: { stream: StreamSource; index: number; onClick(): void }) {
  const enabled = !!stream.url || !!stream.externalUrl;
  const title = firstLine(stream.name) || firstLine(stream.title) || `Source ${index + 1}`;
  const lines = streamDetailLines(stream, title);
  const badges = streamBadges(stream);
  return <button className="nuvio-stream-card" disabled={!enabled} onClick={onClick}><div className="stream-card-copy"><strong>{title}</strong><div className="stream-detail-lines">{lines.map((line, lineIndex) => <span key={`${lineIndex}:${line}`}>{line}</span>)}</div><div className="stream-badges">{badges.map((badge) => <span key={badge}>{badge}</span>)}</div></div><small>{stream.url ? "PLAY" : stream.externalUrl ? "OPEN" : stream.infoHash ? "RESOLVER NEEDED" : "UNAVAILABLE"}</small></button>;
}

function Credit({ label, value }: { label: string; value: string }) { return <div><span>{label}</span><strong>{value}</strong></div>; }
function formatTime(milliseconds: number) { const minutes = Math.floor(milliseconds / 60000); return `${Math.floor(minutes / 60)}h ${minutes % 60}m`; }
const ratingVisuals = [
  { source: "imdb", name: "IMDb", icon: "/rating_imdb.png", color: "#f5c518", format: oneDecimal, wide: true },
  { source: "tmdb", name: "TMDB", icon: "/rating_tmdb.png", color: "#01b4e4", format: whole, wide: false },
  { source: "trakt", name: "Trakt", icon: "/rating_trakt.png", color: "#ed1c24", format: whole, wide: false },
  { source: "letterboxd", name: "Letterboxd", icon: "/rating_letterboxd.png", color: "#00e054", format: oneDecimal, wide: false },
  { source: "mal", name: "MyAnimeList", icon: "/rating_mal.png", color: "#2e51a2", format: oneDecimal, wide: false },
  { source: "tomatoes", name: "Rotten Tomatoes", icon: "/rating_rotten_tomatoes.png", color: "#fa320a", format: percent, wide: false },
  { source: "audience", name: "Audience score", icon: "/rating_audience_score.png", color: "#fa320a", format: percent, wide: false },
  { source: "metacritic", name: "Metacritic", icon: "/rating_metacritic.png", color: "#ffcc33", format: whole, wide: false },
] as const;
function DetailsRatings({ ratings }: { ratings: ExternalRating[] }) {
  const bySource = new Map(ratings.map((rating) => [rating.source.toLowerCase(), rating]));
  return <div className="details-ratings">{ratingVisuals.map((visual) => {
    const rating = bySource.get(visual.source);
    if (!rating) return null;
    return <span className={visual.wide ? "rating-wide" : undefined} style={{ color: visual.color }} key={visual.source} title={visual.name}><img src={visual.icon} alt={visual.name} />{visual.format(rating.value)}</span>;
  })}</div>;
}
function oneDecimal(value: number) { return value.toFixed(1); }
function whole(value: number) { return Math.round(value).toString(); }
function percent(value: number) { return `${Math.round(value)}%`; }
function formatDate(value: string) { const date = new Date(value); return Number.isNaN(date.getTime()) ? value.slice(0, 10) : date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" }); }
function orderSeasons(values: number[]) { return values.sort((left, right) => left === right ? 0 : left === 0 ? 1 : right === 0 ? -1 : left - right); }
function firstEpisode(videos: Video[]) { return [...videos].filter((video) => (video.season ?? 0) > 0 && video.available !== false).sort((left, right) => (left.season ?? 0) - (right.season ?? 0) || (left.episode ?? 0) - (right.episode ?? 0))[0]; }
function firstLine(value?: string) { return value?.split(/\r?\n/).find((line) => line.trim())?.trim(); }
function streamDetailLines(stream: StreamSource, heading: string) {
  let lines = [...new Set([stream.name, stream.description, stream.title]
    .flatMap((value) => (value || "").split(/\r?\n/))
    .map((line) => line.trim())
    .filter((line) => line && line !== heading))];
  const filename = stream.behaviorHints?.filename?.trim();
  if (filename && normalizeStreamLine(filename) !== normalizeStreamLine(heading)) {
    const normalizedFilename = normalizeStreamLine(filename);
    lines = lines.filter((line) => normalizeStreamLine(line) !== normalizedFilename);
    lines.unshift(`▰ ${filename}`);
  }
  lines = lines.filter((line, index, all) => all.findIndex((candidate) => normalizeStreamLine(candidate) === normalizeStreamLine(line)) === index);
  if (stream.behaviorHints?.videoSize && !lines.some((line) => /\b(?:gb|mb)\b/i.test(line))) lines.push(`▣ ${formatBytes(stream.behaviorHints.videoSize)}`);
  if (lines.length === 0) lines.push("No additional stream information");
  return lines;
}
function normalizeStreamLine(value: string) {
  return value
    .replace(/^[^\p{L}\p{N}]+/u, "")
    .replace(/[._\s-]+/g, " ")
    .trim()
    .toLowerCase();
}
function streamBadges(stream: StreamSource) {
  const text = `${stream.name} ${stream.title} ${stream.description} ${stream.behaviorHints?.filename ?? ""}`;
  const rules: Array<[RegExp, string]> = [
    [/\b(2160p|4k)\b/i, "4K"], [/\b1080p\b/i, "1080p"], [/\b720p\b/i, "720p"],
    [/\b(web-?dl|webdl)\b/i, "WebDL"], [/\bbluray\b/i, "BluRay"], [/\bremux\b/i, "REMUX"],
    [/\b(dolby[ .]?vision|dovi|\bdv\b)\b/i, "Dolby Vision"], [/\bhdr10\+?\b/i, "HDR10"], [/\bhdr\b/i, "HDR"],
    [/\b(hevc|h[ .]?265|x265)\b/i, "HEVC"], [/\b(h[ .]?264|x264|avc)\b/i, "H.264"], [/\bav1\b/i, "AV1"],
    [/\btruehd\b/i, "TrueHD"], [/\batmos\b/i, "Atmos"], [/\b7[ .]?1\b/i, "7.1"], [/\b5[ .]?1\b/i, "5.1"],
  ];
  const badges = rules.filter(([pattern]) => pattern.test(text)).map(([, label]) => label);
  const size = stream.behaviorHints?.videoSize;
  if (size && size > 0) badges.push(formatBytes(size));
  return [...new Set(badges)].slice(0, 9);
}
function formatBytes(bytes: number) { const gb = bytes / 1_073_741_824; return gb >= 1 ? `${gb.toFixed(gb >= 10 ? 1 : 2)} GB` : `${(bytes / 1_048_576).toFixed(0)} MB`; }

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { ProgressSnapshot, SettingsSnapshot, StreamSource, Video } from "../bridge/types";
import type { PlayContext } from "./DetailsOverlay";
import { Icon } from "./Icon";
import { resolveNextEpisode, shouldShowNextEpisode } from "../data/nextEpisode";
import { contentKey, removeStreamLink, saveStreamLink } from "../data/streamLinkCache";
import { useClientSettings } from "../data/clientSettings";
import {
  autoplayCandidates,
  selectAutoplayFallback,
  selectPreferredBingeGroup,
  waitForAutoplayWindow,
} from "../data/streamAutoplay";
import { saveBingeGroup } from "../data/bingeGroupCache";
import { applyDebridStreamSettings } from "../data/debridStreams";
import { EpisodeBadge } from "./WatchStatus";

type PlayerTrack = { id: number; kind: "audio" | "sub"; title: string; lang: string; selected: boolean };

type NativePlayerState = {
  active: boolean;
  loading: boolean;
  paused: boolean;
  positionMs: number;
  durationMs: number;
  volume: number;
  muted: boolean;
  audioTrack: number;
  subtitleTrack: number;
  tracks: PlayerTrack[];
  ended: boolean;
  error?: string;
  warning?: string;
};

type PendingValue<T> = { value: T; submittedAt: number };
type SkipSegment = { startMs: number; endMs: number; type: string; provider: string };

export type ActivePlayback = {
  title: string;
  context: PlayContext;
  stream: StreamSource;
};

const emptyState: NativePlayerState = { active: true, loading: true, paused: false, positionMs: 0, durationMs: 0, volume: 100, muted: false, audioTrack: -1, subtitleTrack: -1, tracks: [], ended: false };

export function PlayerPage({ playback, progress, amoled, settings, onBack, onPlayEpisode }: { playback: ActivePlayback; progress: ProgressSnapshot; amoled: boolean; settings?: SettingsSnapshot | null; onBack(): void; onPlayEpisode?(video: Video, stream: StreamSource): Promise<void> | void }) {
  const [state, setState] = useState(emptyState);
  const [fullscreen, setFullscreen] = useState(false);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [trackPicker, setTrackPicker] = useState<"audio" | "sub" | null>(null);
  const [panel, setPanel] = useState<"episodes" | "sources" | null>(null);
  const [sources, setSources] = useState<StreamSource[] | null>(null);
  const [sourceEpisode, setSourceEpisode] = useState<Video | null>(null);
  const [currentStream, setCurrentStream] = useState(playback.stream);
  const [advancing, setAdvancing] = useState(false);
  const [nextDismissed, setNextDismissed] = useState(false);
  const [seekBusy, setSeekBusy] = useState(false);
  const [speeding, setSpeeding] = useState(false);
  const [parentalGuideVisible, setParentalGuideVisible] = useState(false);
  const [pulse, setPulse] = useState<"play" | "pause" | null>(null);
  const client = useClientSettings();
  const confirmedPosition = useRef(0);
  const pulseTimer = useRef<number | null>(null);
  const [preview, setPreview] = useState<{ positionMs: number; x: number; image?: string; exact: boolean } | null>(null);
  const previewTimer = useRef<number | null>(null);
  const previewRef = useRef<typeof preview>(null);
  const previewInFlight = useRef(false);
  const pendingPreviewBucket = useRef<number | null>(null);
  const previewGeneration = useRef(0);
  // Frames are cached per 10s bucket so scrubbing back over the same stretch
  // costs nothing; an uncached remote seek still depends on its server/index.
  const previewCache = useRef(new Map<number, string>());
  const [, setClockTick] = useState(0);
  const [skipSegments, setSkipSegments] = useState<SkipSegment[]>([]);
  const [dismissedSegment, setDismissedSegment] = useState<string | null>(null);
  const hideTimer = useRef<number | null>(null);
  const holdControls = useRef(false);
  const volumeTimer = useRef<number | null>(null);
  const lastPointerActivity = useRef(0);
  const controlsVisibleRef = useRef(true);
  const stateRef = useRef(emptyState);
  const seekingRef = useRef(false);
  const pendingSeek = useRef<PendingValue<number> | null>(null);
  const pendingVolume = useRef<PendingValue<number> | null>(null);
  const pendingPause = useRef<PendingValue<boolean> | null>(null);
  const pendingMute = useRef<PendingValue<boolean> | null>(null);
  const advanceInFlight = useRef(false);
  const autoAdvancedVideo = useRef<string | null>(null);
  const autoSelectedTracksFor = useRef<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  const command = useCallback((method: string, params: Record<string, unknown> = {}) => {
    invoke(method, params).catch(() => undefined);
  }, []);

  const leave = useCallback(async () => {
    if (fullscreen) await invoke("window.setFullscreen", { enabled: false }).catch(() => undefined);
    await invoke("player.stop").catch(() => undefined);
    onBack();
  }, [fullscreen, onBack]);

  const toggleFullscreen = useCallback(async () => {
    const next = !fullscreen;
    await invoke("window.setFullscreen", { enabled: next }).catch(() => undefined);
    setFullscreen(next);
  }, [fullscreen]);

  const revealControls = useCallback(() => {
    const now = performance.now();
    if (now - lastPointerActivity.current < 180) return;
    lastPointerActivity.current = now;
    if (!controlsVisibleRef.current) {
      controlsVisibleRef.current = true;
      setControlsVisible(true);
    }
    if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
    // Resting the pointer on the seek bar or a menu is interaction even though
    // it fires no pointermove, so the deck must not fade out from under it.
    if (holdControls.current) return;
    hideTimer.current = window.setTimeout(() => {
      controlsVisibleRef.current = false;
      setControlsVisible(false);
    }, 2600);
  }, []);

  const beginControlHover = useCallback(() => {
    holdControls.current = true;
    if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
    if (!controlsVisibleRef.current) {
      controlsVisibleRef.current = true;
      setControlsVisible(true);
    }
  }, []);

  const endControlHover = useCallback(() => {
    holdControls.current = false;
    lastPointerActivity.current = 0;
    revealControls();
  }, [revealControls]);

  useEffect(() => {
    let live = true;
    const refresh = () => invoke<NativePlayerState>("player.state").then((next) => {
      if (!live) return;
      const now = performance.now();
      const merged = { ...next };

      if (seekingRef.current) {
        merged.positionMs = stateRef.current.positionMs;
      } else if (pendingSeek.current) {
        const pending = pendingSeek.current;
        const confirmed = Math.abs(next.positionMs - pending.value) < 2500;
        if (confirmed || now - pending.submittedAt > 5000) {
          pendingSeek.current = null;
          setSeekBusy(false);
        } else {
          merged.positionMs = pending.value;
        }
      }

      if (pendingVolume.current) {
        const pending = pendingVolume.current;
        if (Math.abs(next.volume - pending.value) <= 1 || now - pending.submittedAt > 2500) pendingVolume.current = null;
        else merged.volume = pending.value;
      }
      if (pendingPause.current) {
        const pending = pendingPause.current;
        if (next.paused === pending.value || now - pending.submittedAt > 2500) pendingPause.current = null;
        else merged.paused = pending.value;
      }
      if (pendingMute.current) {
        const pending = pendingMute.current;
        if (next.muted === pending.value || now - pending.submittedAt > 2500) pendingMute.current = null;
        else merged.muted = pending.value;
      }

      confirmedPosition.current = next.positionMs;
      stateRef.current = merged;
      setState(merged);
    }).catch(() => undefined);
    refresh();
    const timer = window.setInterval(refresh, 500);
    return () => { live = false; window.clearInterval(timer); };
  }, []);

  // A reused link that has since expired fails with MPV_ERROR_LOADING_FAILED.
  // Drop it so the next attempt re-resolves instead of replaying the dead URL.
  useEffect(() => {
    if (!state.error) return;
    removeStreamLink(
      contentKey(
        playback.context.contentType,
        playback.context.videoId,
        playback.context.contentId,
        playback.context.season,
        playback.context.episode,
      ),
    );
  }, [state.error, playback.context]);

  useEffect(() => {
    setNextDismissed(false);
    setPanel(null);
    setSources(null);
    setSourceEpisode(null);
    setCurrentStream(playback.stream);
    setAdvancing(false);
    advanceInFlight.current = false;
    autoAdvancedVideo.current = null;
    autoSelectedTracksFor.current = null;
    previewGeneration.current += 1;
    pendingPreviewBucket.current = null;
    previewCache.current.clear();
    previewRef.current = null;
    setPreview(null);
  }, [playback.context.videoId]);

  // Nuvio applies language preferences once, after the stream exposes its
  // tracks. Keeping this one-shot prevents state polling from undoing a track
  // the viewer selected manually afterwards.
  useEffect(() => {
    if (
      state.loading ||
      state.tracks.length === 0 ||
      autoSelectedTracksFor.current === playback.context.videoId
    ) return;
    autoSelectedTracksFor.current = playback.context.videoId;

    const audio = state.tracks.filter((track) => track.kind === "audio");
    const subtitles = state.tracks.filter((track) => track.kind === "sub");
    const audioTargets = preferredAudioTargets(settings, playback.context.originalLanguage);
    if ((settings?.preferredAudioLanguage ?? "device") !== "default") {
      const preferredAudio = findTrack(audio, audioTargets);
      if (preferredAudio) command("player.setAudioTrack", { id: preferredAudio.id });
    }

    const subtitleTargets = preferredSubtitleTargets(settings);
    const forcedOnly = settings?.subtitleForcedOnly === true || settings?.preferredSubtitleLanguage === "forced";
    if (forcedOnly) {
      const selectedAudio = audio.find((track) => track.selected) ?? findTrack(audio, audioTargets);
      const targets = subtitleTargets.length
        ? subtitleTargets
        : selectedAudio?.lang ? [selectedAudio.lang] : audioTargets;
      const forced = findTrack(subtitles.filter(isForcedTrack), targets);
      command("player.setSubtitleTrack", { id: forced?.id ?? 0 });
    } else if (subtitleTargets.length > 0) {
      const preferredSubtitle = findTrack(subtitles.filter((track) => !isForcedTrack(track)), subtitleTargets);
      command("player.setSubtitleTrack", { id: preferredSubtitle?.id ?? 0 });
    } else {
      // `none` is an instruction, not merely an absent preference.
      command("player.setSubtitleTrack", { id: 0 });
    }
  }, [state.loading, state.tracks, playback.context.videoId, playback.context.originalLanguage, settings, command]);

  useEffect(() => {
    if (
      state.loading ||
      state.error ||
      settings?.showParentalGuide === false ||
      !playback.context.ageRating
    ) {
      setParentalGuideVisible(false);
      return;
    }
    setParentalGuideVisible(true);
    const timer = window.setTimeout(() => setParentalGuideVisible(false), 4800);
    return () => window.clearTimeout(timer);
  }, [state.loading, state.error, playback.context.videoId, playback.context.ageRating, settings?.showParentalGuide]);

  useEffect(() => {
    let live = true;
    setSkipSegments([]);
    setDismissedSegment(null);
    const context = playback.context;
    if (context.season == null || context.episode == null) return () => { live = false; };
    if (settings?.skipIntro === false) return () => { live = false; };
    invoke<{ segments: SkipSegment[] }>("player.skipSegments", {
      contentId: context.contentId,
      videoId: context.videoId,
      season: context.season,
      episode: context.episode,
    })
      .then((result) => { if (live) setSkipSegments(result.segments); })
      .catch(() => { if (live) setSkipSegments([]); });
    return () => { live = false; };
  }, [playback.context.contentId, playback.context.videoId, playback.context.season, playback.context.episode, settings?.skipIntro]);

  const seek = useCallback((positionMs: number) => {
    const next = Math.max(0, positionMs);
    // Only a real jump warrants a spinner; a nudge lands too fast to matter.
    if (Math.abs(next - confirmedPosition.current) > 4000) setSeekBusy(true);
    pendingSeek.current = { value: next, submittedAt: performance.now() };
    stateRef.current = { ...stateRef.current, positionMs: next };
    setState((current) => ({ ...current, positionMs: next }));
    command("player.seek", { positionMs: next });
  }, [command]);
  const previewSeek = useCallback((positionMs: number) => {
    stateRef.current = { ...stateRef.current, positionMs };
    setState((current) => ({ ...current, positionMs }));
  }, []);
  const seekRelative = useCallback((offsetMs: number) => {
    seek(Math.max(0, stateRef.current.positionMs + offsetMs));
  }, [seek]);
  const togglePause = useCallback(() => {
    const paused = !stateRef.current.paused;
    pendingPause.current = { value: paused, submittedAt: performance.now() };
    stateRef.current = { ...stateRef.current, paused };
    setState((current) => ({ ...current, paused }));
    command("player.togglePause");
  }, [command]);
  const toggleMute = useCallback(() => {
    const muted = !stateRef.current.muted;
    pendingMute.current = { value: muted, submittedAt: performance.now() };
    stateRef.current = { ...stateRef.current, muted };
    setState((current) => ({ ...current, muted }));
    command("player.toggleMute");
  }, [command]);
  const setVolume = useCallback((volume: number) => {
    pendingVolume.current = { value: volume, submittedAt: performance.now() };
    stateRef.current = { ...stateRef.current, volume };
    setState((current) => ({ ...current, volume }));
    if (volumeTimer.current != null) window.clearTimeout(volumeTimer.current);
    volumeTimer.current = window.setTimeout(() => command("player.setVolume", { volume }), 45);
  }, [command]);

  useEffect(() => {
    const timer = window.setInterval(() => setClockTick((tick) => tick + 1), 1000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    document.documentElement.classList.add("player-active");
    document.body.classList.add("player-active");
    revealControls();
    return () => {
      document.documentElement.classList.remove("player-active");
      document.body.classList.remove("player-active");
      if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
      if (volumeTimer.current != null) window.clearTimeout(volumeTimer.current);
      if (pulseTimer.current != null) window.clearTimeout(pulseTimer.current);
      if (previewTimer.current != null) window.clearTimeout(previewTimer.current);
      invoke("player.setSpeed", { speed: 1 }).catch(() => undefined);
    };
  }, [revealControls]);

  useEffect(() => {
    if (state.paused || state.loading || state.error) {
      setControlsVisible(true);
      if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
    } else {
      revealControls();
    }
  }, [state.paused, state.loading, state.error, revealControls]);

  useEffect(() => {
    const keyboard = (event: KeyboardEvent) => {
      if (event.target instanceof HTMLInputElement) return;
      revealControls();
      if (event.code === "Space") { event.preventDefault(); togglePause(); }
      else if (event.code === "ArrowLeft") seekRelative(-10000);
      else if (event.code === "ArrowRight") seekRelative(10000);
      else if (event.key.toLowerCase() === "m") toggleMute();
      else if (event.key.toLowerCase() === "f") toggleFullscreen();
      else if (event.key === "Escape") {
        if (trackPicker) setTrackPicker(null);
        else leave();
      }
    };
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  }, [leave, revealControls, seekRelative, toggleFullscreen, toggleMute, togglePause, trackPicker]);

  const chooseTrack = (kind: "audio" | "sub", id: number) => {
    command(kind === "audio" ? "player.setAudioTrack" : "player.setSubtitleTrack", { id });
    setTrackPicker(null);
  };
  const activeSkipSegment = skipSegments.find((segment) => state.positionMs >= segment.startMs && state.positionMs < segment.endMs);
  const activeSkipKey = activeSkipSegment ? `${activeSkipSegment.type}:${activeSkipSegment.startMs}:${activeSkipSegment.endMs}` : null;
  const showSkip = activeSkipSegment && activeSkipKey !== dismissedSegment;
  const skipActiveSegment = () => {
    if (!activeSkipSegment || !activeSkipKey) return;
    setDismissedSegment(activeSkipKey);
    seek(activeSkipSegment.endMs);
    revealControls();
  };
  const episodes = playback.context.videos ?? [];
  const nextEpisode = resolveNextEpisode(episodes, playback.context.season, playback.context.episode);
  const showNextCard =
    !!nextEpisode &&
    !nextDismissed &&
    !!onPlayEpisode &&
    shouldShowNextEpisode(
      state.positionMs,
      state.durationMs,
      skipSegments.map((segment) => ({ startMs: segment.startMs, endMs: segment.endMs, type: segment.type })),
      {
        nextEpisodeThresholdMode: settings?.nextEpisodeThresholdMode ?? "PERCENTAGE",
        nextEpisodeThresholdPercent: settings?.nextEpisodeThresholdPercent ?? 99,
        nextEpisodeThresholdMinutes: settings?.nextEpisodeThresholdMinutes ?? 2,
      },
    );

  async function openSourcePanel(video?: Video) {
    const targetId = video?.id ?? playback.context.videoId;
    setSourceEpisode(video ?? null);
    setPanel("sources");
    setSources(null);
    try {
      const result = await invoke<{ streams: StreamSource[] }>("content.streams", {
        type: playback.context.contentType,
        id: targetId,
      });
      setSources(applyDebridStreamSettings(result.streams, settings));
    } catch {
      setSources([]);
    }
  }

  async function playSource(stream: StreamSource) {
    setPanel(null);
    const target = sourceEpisode;
    setSourceEpisode(null);
    if (target && onPlayEpisode) {
      setCurrentStream(stream);
      await onPlayEpisode(target, stream);
      return;
    }
    setCurrentStream(stream);
    saveStreamLink(
      contentKey(
        playback.context.contentType,
        playback.context.videoId,
        playback.context.contentId,
        playback.context.season,
        playback.context.episode,
      ),
      stream,
    );

    saveBingeGroup(playback.context.contentId, stream.behaviorHints?.bingeGroup);
    invoke("player.prepare", {
      mediaId: playback.title,
      url: stream.url,
      externalUrl: stream.externalUrl,
      requestHeaders: stream.behaviorHints?.proxyHeaders?.request,
      startPositionMs: stateRef.current.positionMs,
      progress: {
        contentId: playback.context.contentId,
        contentType: playback.context.contentType,
        videoId: playback.context.videoId,
        season: playback.context.season,
        episode: playback.context.episode,
      },
    }).catch(() => undefined);
  }

  async function advanceToEpisode(video: Video) {
    if (!onPlayEpisode || advanceInFlight.current) return;
    advanceInFlight.current = true;
    setAdvancing(true);
    setNextDismissed(true);
    const startedAt = performance.now();
    try {
      const result = await invoke<{ streams: StreamSource[] }>("content.streams", {
        type: playback.context.contentType,
        id: video.id,
      });
      if (!mounted.current) return;
      const candidates = autoplayCandidates(
        applyDebridStreamSettings(result.streams, settings),
        settings,
        true,
      );
      // Nuvio's in-player transition uses the stream that is playing now. The
      // persisted cache is for a later visit to Details, not a substitute for
      // a current stream that supplied no group.
      const rememberedGroup = currentStream.behaviorHints?.bingeGroup;
      const preferred =
        (settings?.autoplayPreferBingeGroup ?? true)
          ? selectPreferredBingeGroup(candidates, rememberedGroup)
          : null;
      if (preferred) {
        setCurrentStream(preferred);
        await onPlayEpisode(video, preferred);
        return;
      }

      // Nuvio lets a preferred group select immediately; the configured wait
      // applies only before First Stream / Regex fallback is evaluated.
      await waitForAutoplayWindow(startedAt, settings?.autoplayTimeoutSeconds);
      if (!mounted.current) return;
      const fallback = selectAutoplayFallback(candidates, settings, true);
      if (fallback) {
        setCurrentStream(fallback);
        await onPlayEpisode(video, fallback);
        return;
      }

      // Manual mode, an invalid/no-match regex, or a strict binge-group miss.
      // Keep the player mounted and show the already-fetched next sources.
      setSourceEpisode(video);
      setSources(result.streams);
      setPanel("sources");
    } catch {
      setSourceEpisode(video);
      setSources([]);
      setPanel("sources");
    } finally {
      advanceInFlight.current = false;
      if (mounted.current) setAdvancing(false);
    }
  }

  useEffect(() => {
    if (
      !state.ended ||
      !settings?.autoplayNextEpisode ||
      !nextEpisode ||
      autoAdvancedVideo.current === playback.context.videoId
    ) {
      return;
    }
    autoAdvancedVideo.current = playback.context.videoId;
    void advanceToEpisode(nextEpisode);
  }, [state.ended, settings?.autoplayNextEpisode, nextEpisode?.id, playback.context.videoId]);

  const audioTracks = state.tracks.filter((track) => track.kind === "audio");
  const subtitleTracks = visibleSubtitleTracks(
    state.tracks.filter((track) => track.kind === "sub"),
    settings,
  );
  const endsAt = endTimeLabel(state);

  const PREVIEW_BUCKET_MS = 10_000;

  /** Closest already-decoded frame, so a pending capture has something to show
   *  instead of an empty box — a wrong-but-near frame beats a spinner. */
  function nearestCached(bucket: number): string | undefined {
    let best: string | undefined;
    let bestGap = Infinity;
    for (const [key, image] of previewCache.current) {
      const gap = Math.abs(key - bucket);
      if (gap < bestGap) {
        bestGap = gap;
        best = image;
      }
    }
    return best;
  }

  function showPreview(next: typeof preview) {
    previewRef.current = next;
    setPreview(next);
  }

  /** Only one decoder request may be active. While the pointer moves, retain
   * just the newest bucket rather than building a slow queue of obsolete HTTP
   * opens behind libmpv. */
  function fetchPreview(bucket: number, generation: number) {
    if (previewInFlight.current) {
      pendingPreviewBucket.current = bucket;
      return;
    }
    previewInFlight.current = true;
    invoke<{ image: string }>("player.thumbnail", { positionMs: bucket })
      .then((result) => {
        if (generation !== previewGeneration.current) return;
        previewCache.current.set(bucket, result.image);
        const current = previewRef.current;
        if (
          current &&
          Math.round(current.positionMs / PREVIEW_BUCKET_MS) * PREVIEW_BUCKET_MS === bucket
        ) {
          showPreview({ ...current, image: result.image, exact: true });
        }
      })
      .catch(() => undefined)
      .finally(() => {
        previewInFlight.current = false;
        const next = pendingPreviewBucket.current;
        pendingPreviewBucket.current = null;
        if (next != null && previewRef.current && !previewCache.current.has(next)) {
          fetchPreview(next, previewGeneration.current);
        }
      });
  }

  function requestPreview(positionMs: number, x: number) {
    if (!client.seekThumbnails || state.durationMs <= 0) return;
    const bucket = Math.round(positionMs / PREVIEW_BUCKET_MS) * PREVIEW_BUCKET_MS;
    const exact = previewCache.current.get(bucket);
    showPreview({ positionMs, x, image: exact ?? nearestCached(bucket), exact: !!exact });
    if (exact) return;
    if (previewTimer.current != null) window.clearTimeout(previewTimer.current);
    previewTimer.current = window.setTimeout(() => {
      fetchPreview(bucket, previewGeneration.current);
    }, 120);
  }

  function clearPreview() {
    if (previewTimer.current != null) window.clearTimeout(previewTimer.current);
    pendingPreviewBucket.current = null;
    showPreview(null);
  }

  function flashPulse(kind: "play" | "pause") {
    setPulse(kind);
    if (pulseTimer.current != null) window.clearTimeout(pulseTimer.current);
    pulseTimer.current = window.setTimeout(() => setPulse(null), 620);
  }

  function onStagePointerDown(event: React.PointerEvent) {
    if (covered) return;
    if (event.button === 2 && settings?.holdToSpeed !== false) {
      event.preventDefault();
      setSpeeding(true);
      command("player.setSpeed", { speed: settings?.holdToSpeedValue ?? 2 });
      return;
    }
    if (event.button !== 0 || !client.clickToPause) return;
    // Only the bare video surface toggles playback; controls handle their own.
    if ((event.target as HTMLElement).closest("button, input, select, a, .player-side-panel, .player-track-menu, .player-next-card")) return;
    flashPulse(stateRef.current.paused ? "play" : "pause");
    togglePause();
  }

  function endSpeeding() {
    if (!speeding) return;
    setSpeeding(false);
    command("player.setSpeed", { speed: 1 });
  }

  const covered = !!state.error || (state.loading && settings?.showLoadingOverlay !== false);
  return <div className={`embedded-player-page${amoled ? " amoled" : ""}${covered ? " is-covered" : controlsVisible ? " controls-visible" : " controls-hidden"}`} onPointerMove={revealControls}
    onPointerDown={(event) => { revealControls(); onStagePointerDown(event); }}
    onPointerUp={endSpeeding}
    onPointerLeave={endSpeeding}
    onPointerCancel={endSpeeding}
    onContextMenu={(event) => event.preventDefault()}>
    <div className="embedded-player-stage" aria-label="Embedded video surface" />
    {((state.loading && settings?.showLoadingOverlay !== false) || state.error) && (
      <div
        className={`player-startup-cover${state.error ? " error" : ""}`}
        style={playback.context.backdrop && !state.error
          ? { backgroundImage: `url("${playback.context.backdrop.replaceAll('"', "%22")}")` }
          : undefined}
      >
        <button
          className="player-glyph player-cover-back"
          title="Back to details"
          aria-label="Back to details"
          onClick={leave}
        >
          <Icon name="back" size={26} />
        </button>
        <div className="player-startup-inner">
          {playback.context.logo && !state.error ? (
            <img className="player-startup-logo" src={playback.context.logo} alt={playback.title} />
          ) : (
            <strong>{state.error ? "This source could not be started" : playback.title}</strong>
          )}
          {state.error ? (
            <>
              <span>{state.error}</span>
              <button className="player-error-action" onClick={() => { void openSourcePanel(); }}>
                Choose another source
              </button>
            </>
          ) : (
            <>
              <span>{episodeLabel(playback.context)}</span>
              <i className="player-startup-pulse" aria-hidden="true"><b /><b /><b /></i>
            </>
          )}
        </div>
      </div>
    )}
    {advancing && (
      <div className="player-next-searching" role="status">
        <i className="loading-spinner" /> Finding the next episode source…
      </div>
    )}
    {parentalGuideVisible && (
      <div className="player-parental-guide" role="status">
        <span>Rated</span>
        <strong>{playback.context.ageRating}</strong>
      </div>
    )}
    {pulse && (
      <div className="player-pulse-glyph" key={pulse} aria-hidden="true">
        <Icon name={pulse === "play" ? "play" : "pause"} size={54} />
      </div>
    )}
    {speeding && (
      <div className="player-speed-flag" role="status">{settings?.holdToSpeedValue ?? 2}x</div>
    )}
    {seekBusy && !state.loading && !state.error && (
      <div className="player-seek-busy" role="status"><i className="loading-spinner" /></div>
    )}
    {state.warning && !state.error && (
      <div className="player-message" role="status">{state.warning}</div>
    )}
    <header
      className="player-overlay-header"
      onPointerEnter={beginControlHover}
      onPointerLeave={endControlHover}
    >
      <button className="player-glyph player-back" title="Back to details" aria-label="Back to details" onClick={leave}><Icon name="back" size={30} /></button>
      <div className="player-title"><span>{episodeLabel(playback.context)}</span><strong>{playback.title}</strong></div>
      <div className="player-header-right">
        {endsAt && <span className="player-ends-at" title="Estimated finish time">Ends at {endsAt}</span>}
        <button className="player-glyph" title="Fullscreen" aria-label="Fullscreen" onClick={toggleFullscreen}><Icon name="fullscreen" size={26} /></button>
      </div>
    </header>
    {showSkip && <button className="player-skip-prompt" onClick={skipActiveSegment}><Icon name="forward" size={21} /><span>{skipLabel(activeSkipSegment.type)}</span></button>}
    <section
      className="player-control-deck"
      onPointerEnter={beginControlHover}
      onPointerLeave={endControlHover}
    >
      <div className="player-timeline-row">
        <span>{formatTime(state.positionMs)}</span>
        <div className="player-seek-wrap">
          {preview && (
            <div className="player-seek-preview" style={{ left: `${preview.x}px` }}>
              <div className="player-seek-preview-frame">
                {preview.image && <img className={preview.exact ? undefined : "is-stale"} src={preview.image} alt="" />}
                {!preview.exact && <i className="loading-spinner" />}
              </div>
              <span>{formatTime(preview.positionMs)}</span>
            </div>
          )}
          <input
            className="player-seek"
            aria-label="Seek"
            type="range"
            min={0}
            max={Math.max(state.durationMs, 1)}
            value={Math.min(state.positionMs, Math.max(state.durationMs, 1))}
            onPointerDown={() => { seekingRef.current = true; }}
            onChange={(event) => previewSeek(Number(event.target.value))}
            onPointerUp={(event) => { seekingRef.current = false; seek(Number(event.currentTarget.value)); }}
            onPointerCancel={() => { seekingRef.current = false; }}
            onKeyUp={(event) => seek(Number(event.currentTarget.value))}
            onPointerMove={(event) => {
              const rect = event.currentTarget.getBoundingClientRect();
              const ratio = Math.min(Math.max((event.clientX - rect.left) / rect.width, 0), 1);
              requestPreview(ratio * state.durationMs, event.clientX - rect.left);
            }}
            onPointerLeave={clearPreview}
          />
        </div>
        <span>{formatTime(state.durationMs)}</span>
      </div>
      <div className="player-control-row">
        <div className="player-control-group player-control-left">
          <button className="player-glyph player-glyph-lg" title={state.paused ? "Play" : "Pause"} aria-label={state.paused ? "Play" : "Pause"} onClick={togglePause}><Icon name={state.paused ? "play" : "pause"} size={36} /></button>
          <button className="player-glyph" title={state.muted ? "Unmute" : "Mute"} aria-label={state.muted ? "Unmute" : "Mute"} onClick={toggleMute}><Icon name={state.muted ? "muted" : "volume"} size={26} /></button>
          <input className="player-volume" aria-label="Volume" type="range" min={0} max={100} value={state.volume} onChange={(event) => setVolume(Number(event.target.value))} />
        </div>
        <div className="player-control-group player-control-right">
          {episodes.length > 0 && (
            <button className={panel === "episodes" ? "player-glyph active" : "player-glyph"} title="Episodes" aria-label="Episodes" onClick={() => setPanel(panel === "episodes" ? null : "episodes")}>
              <Icon name="episodes" size={26} />
            </button>
          )}
          <button className={panel === "sources" ? "player-glyph active" : "player-glyph"} title="Change source" aria-label="Change source" onClick={() => (panel === "sources" ? setPanel(null) : openSourcePanel())}>
            <Icon name="sources" size={26} />
          </button>
          <TrackButton icon="subtitles" label="Subtitles" tracks={subtitleTracks} open={trackPicker === "sub"} allowOff selectedId={state.subtitleTrack} onToggle={() => setTrackPicker(trackPicker === "sub" ? null : "sub")} onChoose={(id) => chooseTrack("sub", id)} />
          <TrackButton icon="audio" label="Audio track" tracks={audioTracks} open={trackPicker === "audio"} selectedId={state.audioTrack} onToggle={() => setTrackPicker(trackPicker === "audio" ? null : "audio")} onChoose={(id) => chooseTrack("audio", id)} />
        </div>
      </div>
    </section>
    {panel && (
      <PlayerSidePanel
        title={panel === "episodes" ? (playback.context.showName ?? "Episodes") : (sourceEpisode?.title || "Sources")}
        onClose={() => setPanel(null)}
      >
        {panel === "episodes" ? (
          <EpisodeList
            episodes={episodes}
            contentId={playback.context.contentId}
            currentId={playback.context.videoId}
            currentSeason={playback.context.season}
            progress={progress}
            onPick={(video) => { setPanel(null); void advanceToEpisode(video); }}
          />
        ) : (
          <SourceList sources={sources} showFileSize={settings?.showFileSizeBadges !== false} onPick={playSource} />
        )}
      </PlayerSidePanel>
    )}
    {showNextCard && nextEpisode && (
      <aside className="player-next-card">
        <button className="player-next-dismiss" title="Dismiss" aria-label="Dismiss" onClick={() => setNextDismissed(true)}>
          <Icon name="close" size={18} />
        </button>
        <span className="player-next-eyebrow">Up next</span>
        <div className="player-next-body">
          {nextEpisode.thumbnail ? (
            <img src={nextEpisode.thumbnail} alt="" />
          ) : (
            <div className="player-next-placeholder"><Icon name="play" size={22} /></div>
          )}
          <div>
            <small>S{nextEpisode.season ?? 0} E{nextEpisode.episode ?? 1}</small>
            <strong>{nextEpisode.title || "Episode " + (nextEpisode.episode ?? 1)}</strong>
          </div>
        </div>
        <button className="player-next-play" onClick={() => { void advanceToEpisode(nextEpisode); }}>
          <Icon name="play" size={17} />Play next episode
        </button>
      </aside>
    )}
  </div>;
}

function PlayerSidePanel({ title, onClose, children }: { title: string; onClose(): void; children: React.ReactNode }) {
  return (
    <>
      <button className="player-panel-scrim" aria-label="Close panel" onClick={onClose} />
      <aside className="player-side-panel">
        <header>
          <h2>{title}</h2>
          <button className="player-glyph" title="Close" aria-label="Close" onClick={onClose}><Icon name="close" size={22} /></button>
        </header>
        <div className="player-side-panel-body">{children}</div>
      </aside>
    </>
  );
}

function EpisodeList({ episodes, contentId, currentId, currentSeason, progress, onPick }: { episodes: Video[]; contentId: string; currentId: string; currentSeason?: number; progress: ProgressSnapshot; onPick(video: Video): void }) {
  const seasons = [...new Set(episodes.map((video) => video.season ?? 0))]
    .filter((season) => season > 0)
    .sort((left, right) => left - right);
  const [season, setSeason] = useState(currentSeason ?? seasons[0] ?? 1);
  const visible = episodes.filter((video) => (video.season ?? 0) === season);
  return (
    <div className="player-episode-list">
      {seasons.length > 1 && (
        <label className="player-season-select">
          <span>Season</span>
          <select value={season} onChange={(event) => setSeason(Number(event.target.value))}>
            {seasons.map((option) => (
              <option key={option} value={option}>Season {option}</option>
            ))}
          </select>
        </label>
      )}
      {visible.map((video) => (
        <button
          key={video.id}
          className={video.id === currentId ? "active" : undefined}
          disabled={video.available === false}
          onClick={() => onPick(video)}
        >
          <div className="player-episode-thumb">
            {video.thumbnail ? (
              <img src={video.thumbnail} alt="" />
            ) : (
              <span className="player-episode-placeholder"><Icon name="play" size={16} /></span>
            )}
            <EpisodeBadge contentId={contentId} videoId={video.id} season={video.season} episode={video.episode} snapshot={progress} />
          </div>
          <span>
            <small>S{video.season ?? 0} E{video.episode ?? 1}{video.id === currentId ? " \u00b7 Playing" : ""}</small>
            <strong>{video.title || "Episode " + (video.episode ?? 1)}</strong>
          </span>
        </button>
      ))}
    </div>
  );
}

function SourceList({ sources, showFileSize, onPick }: { sources: StreamSource[] | null; showFileSize: boolean; onPick(stream: StreamSource): void }) {
  if (!sources) return <div className="player-panel-loading"><i className="loading-spinner" />Finding sources\u2026</div>;
  if (sources.length === 0) return <div className="player-panel-empty">No sources returned.</div>;
  return (
    <div className="player-source-list">
      {sources.map((stream, index) => (
        <button key={stream.addonId + ":" + index} disabled={!stream.url && !stream.externalUrl} onClick={() => onPick(stream)}>
          <strong>{firstLine(stream.name) || firstLine(stream.title) || "Source " + (index + 1)}</strong>
          <span>{stream.addonName}{showFileSize && stream.behaviorHints?.videoSize ? " \u00b7 " + formatBytes(stream.behaviorHints.videoSize) : ""}</span>
        </button>
      ))}
    </div>
  );
}

function firstLine(value?: string) {
  return value?.split(/\r?\n/).find((line) => line.trim())?.trim();
}

function formatBytes(bytes: number) {
  const gb = bytes / 1_073_741_824;
  return gb >= 1 ? gb.toFixed(gb >= 10 ? 1 : 2) + " GB" : (bytes / 1_048_576).toFixed(0) + " MB";
}

/** A glyph that opens a list of tracks; mpv reports -1/"no" for "off". */
function TrackButton({ icon, label, tracks, open, selectedId, allowOff, onToggle, onChoose }: {
  icon: "audio" | "subtitles";
  label: string;
  tracks: PlayerTrack[];
  open: boolean;
  selectedId: number;
  allowOff?: boolean;
  onToggle(): void;
  onChoose(id: number): void;
}) {
  const disabled = tracks.length === 0 && !allowOff;
  return (
    <div className="player-track-control">
      {open && (
        <div className="player-track-menu" role="menu">
          <header>{label}</header>
          {allowOff && (
            <button className={selectedId <= 0 ? "active" : undefined} onClick={() => onChoose(0)}>
              Off
            </button>
          )}
          {tracks.map((track) => (
            <button
              key={track.id}
              className={track.id === selectedId ? "active" : undefined}
              onClick={() => onChoose(track.id)}
            >
              {trackLabel(track)}
            </button>
          ))}
          {tracks.length === 0 && <span className="player-track-empty">None available</span>}
        </div>
      )}
      <button
        className={open ? "player-glyph active" : "player-glyph"}
        title={label}
        aria-label={label}
        aria-expanded={open}
        disabled={disabled}
        onClick={onToggle}
      >
        <Icon name={icon} size={26} />
      </button>
    </div>
  );
}

function trackLabel(track: PlayerTrack) {
  const parts = [track.title, track.lang && track.lang.toUpperCase()].filter(Boolean);
  return parts.length ? parts.join(" · ") : `Track ${track.id}`;
}

const LANGUAGE_ALIASES: Record<string, string> = {
  eng: "en", spa: "es", fre: "fr", fra: "fr", ger: "de", deu: "de",
  ita: "it", por: "pt", rus: "ru", jpn: "ja", kor: "ko", chi: "zh",
  zho: "zh", dut: "nl", nld: "nl", swe: "sv", nor: "no", dan: "da",
  fin: "fi", pol: "pl", tur: "tr", ara: "ar", heb: "he", hin: "hi",
  und: "", unknown: "",
};

function normalizeLanguage(value?: string | null): string {
  const normalized = (value ?? "")
    .trim()
    .toLowerCase()
    .replaceAll("_", "-");
  return LANGUAGE_ALIASES[normalized] ?? normalized;
}

function languageMatches(trackLanguage: string | undefined, targetLanguage: string) {
  const track = normalizeLanguage(trackLanguage);
  const target = normalizeLanguage(targetLanguage);
  if (!track || !target) return false;
  return track === target || track.split("-")[0] === target.split("-")[0];
}

function deviceLanguageTargets() {
  return [...new Set((navigator.languages?.length ? navigator.languages : [navigator.language])
    .map(normalizeLanguage)
    .filter(Boolean))];
}

function optionalLanguage(value?: string) {
  const normalized = normalizeLanguage(value);
  return normalized && !["none", "default", "device", "original", "forced"].includes(normalized)
    ? normalized
    : null;
}

function preferredAudioTargets(settings?: SettingsSnapshot | null, originalLanguage?: string) {
  const primary = normalizeLanguage(settings?.preferredAudioLanguage || "device");
  const secondary = optionalLanguage(settings?.secondaryAudioLanguage);
  let targets: string[];
  if (primary === "default") targets = [];
  else if (primary === "device") targets = deviceLanguageTargets();
  else if (primary === "original") {
    const original = optionalLanguage(originalLanguage);
    targets = original ? [original] : deviceLanguageTargets();
  } else targets = optionalLanguage(primary) ? [primary] : [];
  if (secondary) targets.push(secondary);
  return [...new Set(targets)];
}

function preferredSubtitleTargets(settings?: SettingsSnapshot | null) {
  const primary = normalizeLanguage(settings?.preferredSubtitleLanguage || "none");
  const secondary = optionalLanguage(settings?.secondarySubtitleLanguage);
  let targets = primary === "device"
    ? deviceLanguageTargets()
    : optionalLanguage(primary) ? [primary] : [];
  if (secondary) targets.push(secondary);
  return [...new Set(targets)];
}

function findTrack(tracks: PlayerTrack[], targets: string[]) {
  for (const target of targets) {
    const match = tracks.find((track) =>
      languageMatches(track.lang, target) || languageMatches(track.title, target));
    if (match) return match;
  }
  return undefined;
}

function isForcedTrack(track: PlayerTrack) {
  const text = `${track.title} ${track.lang}`.toLowerCase();
  return normalizeLanguage(track.lang) === "forced" || text.includes("forced") || (text.includes("songs") && text.includes("sign"));
}

function visibleSubtitleTracks(tracks: PlayerTrack[], settings?: SettingsSnapshot | null) {
  let visible = settings?.subtitleForcedOnly ? tracks.filter(isForcedTrack) : tracks;
  if (!settings?.subtitlePreferredLanguagesOnly) return visible;
  const targets = preferredSubtitleTargets(settings);
  if (targets.length === 0) return [];
  visible = visible.filter((track) => targets.some((target) =>
    languageMatches(track.lang, target) || languageMatches(track.title, target)));
  return visible;
}

/** Wall-clock time the title finishes, from the remaining runtime. */
function endTimeLabel(state: NativePlayerState) {
  const remaining = state.durationMs - state.positionMs;
  if (!Number.isFinite(remaining) || remaining <= 0 || state.durationMs <= 0) return null;
  return new Date(Date.now() + remaining).toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
}

function formatTime(milliseconds: number) {
  if (!Number.isFinite(milliseconds) || milliseconds <= 0) return "0:00";
  const seconds = Math.floor(milliseconds / 1000);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;
  return hours ? `${hours}:${String(minutes).padStart(2, "0")}:${String(remainder).padStart(2, "0")}` : `${minutes}:${String(remainder).padStart(2, "0")}`;
}

function episodeLabel(context: PlayContext) {
  return context.season != null && context.episode != null ? `Season ${context.season} · Episode ${context.episode}` : "Movie";
}

function skipLabel(type: string) {
  const normalized = type.toLowerCase();
  if (["intro", "op", "mixed-op"].includes(normalized)) return "Skip intro";
  if (["outro", "ed", "mixed-ed", "credits", "ending"].includes(normalized)) return "Skip outro";
  if (normalized === "recap") return "Skip recap";
  return "Skip";
}

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { SettingsSnapshot } from "../bridge/types";
import type { PlayContext } from "./DetailsOverlay";
import { Icon } from "./Icon";

type NativePlayerState = {
  active: boolean;
  loading: boolean;
  paused: boolean;
  positionMs: number;
  durationMs: number;
  volume: number;
  muted: boolean;
  error?: string;
};

type PendingValue<T> = { value: T; submittedAt: number };
type SkipSegment = { startMs: number; endMs: number; type: string; provider: string };

export type ActivePlayback = { title: string; context: PlayContext };

const emptyState: NativePlayerState = { active: true, loading: true, paused: false, positionMs: 0, durationMs: 0, volume: 100, muted: false };

export function PlayerPage({ playback, amoled, onBack }: { playback: ActivePlayback; amoled: boolean; onBack(): void }) {
  const [state, setState] = useState(emptyState);
  const [fullscreen, setFullscreen] = useState(false);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [trackNotice, setTrackNotice] = useState<string | null>(null);
  const [skipSegments, setSkipSegments] = useState<SkipSegment[]>([]);
  const [dismissedSegment, setDismissedSegment] = useState<string | null>(null);
  const hideTimer = useRef<number | null>(null);
  const volumeTimer = useRef<number | null>(null);
  const lastPointerActivity = useRef(0);
  const controlsVisibleRef = useRef(true);
  const stateRef = useRef(emptyState);
  const seekingRef = useRef(false);
  const pendingSeek = useRef<PendingValue<number> | null>(null);
  const pendingVolume = useRef<PendingValue<number> | null>(null);
  const pendingPause = useRef<PendingValue<boolean> | null>(null);
  const pendingMute = useRef<PendingValue<boolean> | null>(null);

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
    hideTimer.current = window.setTimeout(() => {
      controlsVisibleRef.current = false;
      setControlsVisible(false);
    }, 2600);
  }, []);

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
        if (confirmed || now - pending.submittedAt > 5000) pendingSeek.current = null;
        else merged.positionMs = pending.value;
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

      stateRef.current = merged;
      setState(merged);
    }).catch(() => undefined);
    refresh();
    const timer = window.setInterval(refresh, 500);
    return () => { live = false; window.clearInterval(timer); };
  }, []);

  useEffect(() => {
    let live = true;
    setSkipSegments([]);
    setDismissedSegment(null);
    const context = playback.context;
    if (context.season == null || context.episode == null) return () => { live = false; };
    invoke<SettingsSnapshot>("settings.load")
      .then((settings) => settings.skipIntro
        ? invoke<{ segments: SkipSegment[] }>("player.skipSegments", {
            contentId: context.contentId,
            videoId: context.videoId,
            season: context.season,
            episode: context.episode,
          })
        : { segments: [] })
      .then((result) => { if (live) setSkipSegments(result.segments); })
      .catch(() => { if (live) setSkipSegments([]); });
    return () => { live = false; };
  }, [playback.context.contentId, playback.context.videoId, playback.context.season, playback.context.episode]);

  const seek = useCallback((positionMs: number) => {
    const next = Math.max(0, positionMs);
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
    document.documentElement.classList.add("player-active");
    document.body.classList.add("player-active");
    revealControls();
    return () => {
      document.documentElement.classList.remove("player-active");
      document.body.classList.remove("player-active");
      if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
      if (volumeTimer.current != null) window.clearTimeout(volumeTimer.current);
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
      else if (event.key === "Escape") leave();
    };
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  }, [leave, revealControls, seekRelative, toggleFullscreen, toggleMute, togglePause]);

  const changeTrack = (kind: "audio" | "subtitle") => {
    setTrackNotice(kind === "audio" ? "Switching audio track…" : "Switching subtitles…");
    command(kind === "audio" ? "player.cycleAudio" : "player.cycleSubtitle");
    window.setTimeout(() => setTrackNotice(null), 1400);
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
  return <div className={`embedded-player-page${amoled ? " amoled" : ""}${controlsVisible ? " controls-visible" : ""}`} onPointerMove={revealControls} onPointerDown={revealControls}>
    <div className="embedded-player-stage" aria-label="Embedded video surface" />
    {(state.loading || state.error) && <div className={`player-startup-cover${state.error ? " error" : ""}`}>{state.loading && <i className="loading-spinner" />}<strong>{state.error ? "This source could not be started" : "Loading video…"}</strong><span>{state.error ?? playback.title}</span></div>}
    <header className="player-overlay-header">
      <button className="player-icon-button player-back" title="Back to details" aria-label="Back to details" onClick={leave}><Icon name="back" size={24} /></button>
      <div className="player-title"><span>{episodeLabel(playback.context)}</span><strong>{playback.title}</strong></div>
    </header>
    {showSkip && <button className="player-skip-prompt" onClick={skipActiveSegment}><Icon name="forward" size={21} /><span>{skipLabel(activeSkipSegment.type)}</span></button>}
    <section className="player-control-deck">
      {trackNotice && <div className="player-message">{trackNotice}</div>}
      <div className="player-timeline-row"><span>{formatTime(state.positionMs)}</span><input className="player-seek" aria-label="Seek" type="range" min={0} max={Math.max(state.durationMs, 1)} value={Math.min(state.positionMs, Math.max(state.durationMs, 1))} onPointerDown={() => { seekingRef.current = true; }} onChange={(event) => previewSeek(Number(event.target.value))} onPointerUp={(event) => { seekingRef.current = false; seek(Number(event.currentTarget.value)); }} onPointerCancel={() => { seekingRef.current = false; }} onKeyUp={(event) => seek(Number(event.currentTarget.value))} /><span>{formatTime(state.durationMs)}</span></div>
      <div className="player-control-row">
        <div className="player-control-group player-control-left"><button className="player-icon-button" title={state.muted ? "Unmute" : "Mute"} onClick={toggleMute}><Icon name={state.muted ? "muted" : "volume"} size={20} /></button><input className="player-volume" aria-label="Volume" type="range" min={0} max={100} value={state.volume} onChange={(event) => setVolume(Number(event.target.value))} /></div>
        <div className="player-control-group player-control-center"><button className="player-icon-button" title="Back 10 seconds" onClick={() => seekRelative(-10000)}><Icon name="rewind" size={24} /></button><button className="player-play-button" title={state.paused ? "Play" : "Pause"} onClick={togglePause}><Icon name={state.paused ? "play" : "pause"} size={27} /></button><button className="player-icon-button" title="Forward 10 seconds" onClick={() => seekRelative(10000)}><Icon name="forward" size={24} /></button></div>
        <div className="player-control-group player-control-right"><button className={`player-option-button${trackNotice?.includes("audio") ? " switching" : ""}`} title="Cycle audio track" onClick={() => changeTrack("audio")}><Icon name="audio" size={18} /><span>Audio</span></button><button className={`player-option-button${trackNotice?.includes("subtitle") ? " switching" : ""}`} title="Cycle subtitle track" onClick={() => changeTrack("subtitle")}><Icon name="subtitles" size={18} /><span>CC</span></button><button className="player-icon-button" title="Fullscreen" onClick={toggleFullscreen}><Icon name="fullscreen" size={21} /></button></div>
      </div>
    </section>
  </div>;
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

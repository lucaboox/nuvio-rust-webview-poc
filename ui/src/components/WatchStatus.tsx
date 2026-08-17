import type { ContentMeta, ProgressSnapshot, ResumePoint } from "../bridge/types";
import { Icon } from "./Icon";
import { getWatchedOverride, useWatchedOverrides, watchedKey } from "../data/watchedOverrides";

export type WatchState = { watched: boolean; percent?: number };

export function watchStateForContent(item: Pick<ContentMeta, "id" | "contentType">, snapshot: ProgressSnapshot): WatchState | null {
  const latest = entriesFor(snapshot, item.id)[0];
  const parentWatched = snapshot.watchedItems.some((watched) => watched.contentId === item.id && watched.season == null && watched.episode == null);
  const watched = parentWatched || (!isSeries(item.contentType) && !!latest && isCompleted(latest));
  if (watched) return { watched: true };
  if (!latest || isCompleted(latest)) return null;
  const percent = progressPercent(latest);
  return percent > 0 ? { watched: false, percent } : null;
}

export function watchStateForEpisode(contentId: string, season: number | undefined, episode: number | undefined, videoId: string, snapshot: ProgressSnapshot): WatchState | null {
  const watched = snapshot.watchedItems.some((item) => item.contentId === contentId && item.season === season && item.episode === episode);
  const progress = entriesFor(snapshot, contentId).find((entry) => entry.videoId === videoId || (entry.season === season && entry.episode === episode));
  if (watched || (progress && isCompleted(progress))) return { watched: true };
  if (!progress) return null;
  const percent = progressPercent(progress);
  return percent > 0 ? { watched: false, percent } : null;
}

/**
 * The resume point for one specific video.
 *
 * `progress.resume` only returns the newest entry for the whole title, so
 * picking any other part-watched episode used to start from zero. The full
 * snapshot is already in memory, so match on the episode itself.
 */
export function resumeForVideo(
  snapshot: ProgressSnapshot,
  contentId: string,
  videoId: string,
  season?: number,
  episode?: number,
): ResumePoint | null {
  const candidates = entriesFor(snapshot, contentId).filter(
    (entry) =>
      entry.videoId === videoId ||
      (season != null && episode != null && entry.season === season && entry.episode === episode),
  );
  const newest = candidates[0];
  if (!newest || isCompleted(newest)) return null;
  return progressPercent(newest) > 0 ? newest : null;
}

/** Corner badge for an episode thumbnail: "WATCHED" or "42m left". */
export function EpisodeBadge({
  contentId, videoId, season, episode, snapshot,
}: {
  contentId: string; videoId: string; season?: number; episode?: number; snapshot: ProgressSnapshot;
}) {
  useWatchedOverrides();
  const override = getWatchedOverride(watchedKey(contentId, season, episode));
  const state = watchStateForEpisode(contentId, season, episode, videoId, snapshot);
  // An optimistic flip wins until the snapshot confirms it.
  if (override === true) return <span className="episode-badge watched">Watched</span>;
  if (override === false) return null;
  if (!state) return null;
  if (state.watched) return <span className="episode-badge watched">Watched</span>;
  const entry = resumeForVideo(snapshot, contentId, videoId, season, episode);
  if (!entry) return null;
  return <span className="episode-badge">{formatRemaining(remainingMs(entry))} left</span>;
}

export function formatRemaining(milliseconds: number) {
  const minutes = Math.max(1, Math.round(milliseconds / 60000));
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours <= 0) return `${minutes}m`;
  return rest > 0 ? `${hours}h ${rest}m` : `${hours}h`;
}

/** Newest unfinished resume point across a whole title. */
export function latestResumeFor(snapshot: ProgressSnapshot, contentId: string): ResumePoint | null {
  const newest = entriesFor(snapshot, contentId).find((entry) => !isCompleted(entry));
  return newest && progressPercent(newest) > 0 ? newest : null;
}

export function remainingMs(entry: ResumePoint) {
  return Math.max(0, entry.durationMs - entry.positionMs);
}

export function WatchStatus({ state }: { state: WatchState | null }) {
  if (!state) return null;
  return <>{state.watched && <span className="watch-status watched" aria-label="Watched" title="Watched"><Icon name="eye" size={20} /></span>}{!state.watched && state.percent != null && <span className="watch-progress-track"><i style={{ width: `${state.percent}%` }} /></span>}</>;
}

function entriesFor(snapshot: ProgressSnapshot, id: string) { return snapshot.entries.filter((entry) => entry.contentId === id).sort((left, right) => right.lastWatched - left.lastWatched); }
function progressPercent(entry: ResumePoint) { return entry.durationMs > 0 ? Math.max(0, Math.min(100, entry.positionMs / entry.durationMs * 100)) : 0; }
function isCompleted(entry: ResumePoint) { return progressPercent(entry) >= 90; }
function isSeries(type: string) { return ["series", "show", "tv", "tvshow", "anime"].includes(type.toLowerCase()); }

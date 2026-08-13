import type { ContentMeta, ProgressSnapshot, ResumePoint, Video } from "../bridge/types";
import { Icon } from "./Icon";
import { getWatchedOverride, useWatchedOverrides, watchedKey } from "../data/watchedOverrides";

export type WatchState = { watched: boolean; percent?: number };
export type ContinueWatchingCard = { item: ContentMeta; video?: Video; progress?: ResumePoint; nextUp: boolean; lastWatched: number };

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

export function buildContinueWatching(snapshot: ProgressSnapshot, metadata: ContentMeta[]): ContinueWatchingCard[] {
  const metaById = new Map(metadata.map((item) => [item.id, item]));
  const contentIds = new Set([...snapshot.entries.map((entry) => entry.contentId), ...snapshot.watchedItems.map((item) => item.contentId)]);
  const cards: ContinueWatchingCard[] = [];
  for (const contentId of contentIds) {
    const item = metaById.get(contentId);
    if (!item) continue;
    const entries = entriesFor(snapshot, contentId);
    const resumable = entries.find((entry) => progressPercent(entry) > 0 && !isCompleted(entry));
    if (resumable) {
      cards.push({ item, video: findVideo(item, resumable), progress: resumable, nextUp: false, lastWatched: resumable.lastWatched });
      continue;
    }
    if (!isSeries(item.contentType)) continue;
    const completed = completedEpisodeSeed(snapshot, item, entries);
    if (!completed) continue;
    const next = nextReleasedVideo(item.videos, completed.season, completed.episode);
    if (next) cards.push({ item, video: next, nextUp: true, lastWatched: completed.lastWatched });
  }
  return cards.sort((left, right) => right.lastWatched - left.lastWatched).slice(0, 20);
}

export function WatchStatus({ state }: { state: WatchState | null }) {
  if (!state) return null;
  return <>{state.watched && <span className="watch-status watched"><Icon name="check" size={24} /></span>}{!state.watched && state.percent != null && <span className="watch-progress-track"><i style={{ width: `${state.percent}%` }} /></span>}</>;
}

export function remainingLabel(progress?: ResumePoint) {
  if (!progress || progress.durationMs <= 0) return "Continue";
  const minutes = Math.max(1, Math.ceil((progress.durationMs - progress.positionMs) / 60000));
  const hours = Math.floor(minutes / 60); const rest = minutes % 60;
  return hours > 0 ? `${hours}h${rest > 0 ? ` ${rest}m` : ""} left` : `${minutes}m left`;
}

export function progressForCard(card: ContinueWatchingCard) { return card.progress ? progressPercent(card.progress) : 0; }

function entriesFor(snapshot: ProgressSnapshot, id: string) { return snapshot.entries.filter((entry) => entry.contentId === id).sort((left, right) => right.lastWatched - left.lastWatched); }
function progressPercent(entry: ResumePoint) { return entry.durationMs > 0 ? Math.max(0, Math.min(100, entry.positionMs / entry.durationMs * 100)) : 0; }
function isCompleted(entry: ResumePoint) { return progressPercent(entry) >= 90; }
function isSeries(type: string) { return ["series", "show", "tv", "tvshow", "anime"].includes(type.toLowerCase()); }
function findVideo(item: ContentMeta, entry: ResumePoint) { return item.videos.find((video) => video.id === entry.videoId) || item.videos.find((video) => video.season === entry.season && video.episode === entry.episode); }
function completedEpisodeSeed(snapshot: ProgressSnapshot, item: ContentMeta, entries: ResumePoint[]) {
  const candidates = [
    ...entries.filter((entry) => entry.season != null && entry.episode != null && isCompleted(entry)).map((entry) => ({ season: entry.season!, episode: entry.episode!, lastWatched: entry.lastWatched })),
    ...snapshot.watchedItems.filter((watched) => watched.contentId === item.id && watched.season != null && watched.episode != null).map((watched) => ({ season: watched.season!, episode: watched.episode!, lastWatched: watched.watchedAt })),
  ];
  return candidates.sort((left, right) => right.season - left.season || right.episode - left.episode || right.lastWatched - left.lastWatched)[0];
}
function nextReleasedVideo(videos: Video[], season: number, episode: number) {
  const now = Date.now();
  return [...videos].filter((video) => (video.season ?? 0) > 0 && video.available !== false && (!video.released || new Date(video.released).getTime() <= now)).sort((left, right) => (left.season ?? 0) - (right.season ?? 0) || (left.episode ?? 0) - (right.episode ?? 0)).find((video) => (video.season ?? 0) > season || ((video.season ?? 0) === season && (video.episode ?? 0) > episode));
}

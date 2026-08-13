import type { SettingsSnapshot, Video } from "../bridge/types";

export type SkipInterval = { startMs: number; endMs: number; type: string };

/** Nuvio's `PlayerNextEpisodeRules.OUTRO_SEGMENT_TYPES`. */
const OUTRO_TYPES = new Set(["outro", "ed", "mixed-ed"]);

/** Mirrors `resolveNextEpisode`: the next entry in season/episode order. */
export function resolveNextEpisode(
  videos: Video[],
  season?: number,
  episode?: number,
): Video | null {
  if (season == null || episode == null) return null;
  const sorted = videos
    .filter((video) => video.season != null && video.episode != null)
    .sort(
      (left, right) =>
        (left.season ?? 0) - (right.season ?? 0) ||
        (left.episode ?? 0) - (right.episode ?? 0),
    );
  const index = sorted.findIndex(
    (video) => video.season === season && video.episode === episode,
  );
  return index < 0 ? null : (sorted[index + 1] ?? null);
}

/**
 * Mirrors `shouldShowNextEpisodeCard`.
 *
 * The outro branch is the subtle part: when an outro ends close to the file's
 * end, waiting for the configured threshold would show the card too late, so
 * Nuvio fires at the *earliest outro start* instead.
 */
export function shouldShowNextEpisode(
  positionMs: number,
  durationMs: number,
  skipIntervals: SkipInterval[],
  settings: Pick<
    SettingsSnapshot,
    | "nextEpisodeThresholdMode"
    | "nextEpisodeThresholdPercent"
    | "nextEpisodeThresholdMinutes"
  >,
): boolean {
  if (durationMs <= 0) return false;

  const byPercent = settings.nextEpisodeThresholdMode !== "MINUTES_BEFORE_END";
  const percent = clamp(settings.nextEpisodeThresholdPercent ?? 99, 97, 100);
  const minutes = clamp(settings.nextEpisodeThresholdMinutes ?? 2, 0, 3.5);
  const reachedThreshold = () =>
    byPercent
      ? positionMs / durationMs >= percent / 100
      : durationMs - positionMs <= minutes * 60_000;

  const outros = skipIntervals.filter((segment) =>
    OUTRO_TYPES.has(segment.type.toLowerCase()),
  );
  if (outros.length === 0) return reachedThreshold();

  const latestOutroEnd = Math.max(...outros.map((segment) => segment.endMs));
  const postOutroGap = durationMs - latestOutroEnd;
  const thresholdMs = byPercent
    ? (1 - percent / 100) * durationMs
    : minutes * 60_000;

  if (postOutroGap > thresholdMs) return reachedThreshold();
  return positionMs >= Math.min(...outros.map((segment) => segment.startMs));
}

function clamp(value: number, low: number, high: number) {
  return Math.min(Math.max(value, low), high);
}

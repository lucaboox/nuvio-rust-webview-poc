import type { ProgressSnapshot, Video } from "../bridge/types";

/** Episode represented by the series' Resume / Next Up action. */
export function currentSeriesVideo(
  videos: Video[],
  contentId: string,
  snapshot: ProgressSnapshot,
  selectedVideoId?: string,
  defaultVideoId?: string,
) {
  const episodes = [...videos]
    .filter((video) => (video.season ?? 0) > 0)
    .sort(
      (left, right) =>
        (left.season ?? 0) - (right.season ?? 0) ||
        (left.episode ?? 0) - (right.episode ?? 0),
    );
  const selected = episodes.find((video) => video.id === selectedVideoId);
  const resumable = snapshot.entries
    .filter((entry) => entry.contentId === contentId && isResumable(entry))
    .sort((left, right) => right.lastWatched - left.lastWatched)
    .map((entry) => ({
      entry,
      video: episodes.find(
        (video) =>
          video.id === entry.videoId ||
          (video.season === entry.season && video.episode === entry.episode),
      ),
    }))
    .filter((item): item is { entry: typeof item.entry; video: Video } =>
      Boolean(item.video),
    );
  const selectedResume = resumable.find(
    (item) => item.video.id === selected?.id,
  );
  const resume = selectedResume ?? resumable[0];
  if (resume) return resume.video;

  const lastWatchedIndex = episodes.reduce((last, video, index) => {
    const explicitlyWatched = snapshot.watchedItems.some(
      (item) =>
        item.contentId === contentId &&
        item.season === video.season &&
        item.episode === video.episode,
    );
    const completedProgress = snapshot.entries.some(
      (entry) =>
        entry.contentId === contentId &&
        (entry.videoId === video.id ||
          (entry.season === video.season && entry.episode === video.episode)) &&
        completion(entry) >= 0.9,
    );
    return explicitlyWatched || completedProgress ? index : last;
  }, -1);
  const next = episodes[lastWatchedIndex + 1];
  if (lastWatchedIndex >= 0 && next) return next;

  return (
    selected ??
    episodes.find((video) => video.id === defaultVideoId) ??
    episodes[0] ??
    videos[0]
  );
}

function completion(entry: { positionMs: number; durationMs: number }) {
  return entry.durationMs > 0 ? entry.positionMs / entry.durationMs : 0;
}

function isResumable(entry: { positionMs: number; durationMs: number }) {
  const value = completion(entry);
  return value > 0 && value < 0.9;
}

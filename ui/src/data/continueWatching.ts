import type {
  ContentMeta,
  ProgressSnapshot,
  ResumePoint,
  SettingsSnapshot,
  Video,
} from "../bridge/types";

export type ContinueWatchingPreferences = Pick<
  SettingsSnapshot,
  | "continueWatchingVisible"
  | "continueWatchingStyle"
  | "continueWatchingUpNextFromFurthestEpisode"
  | "continueWatchingUseEpisodeThumbnails"
  | "continueWatchingShowUnairedNextUp"
  | "continueWatchingBlurNextUp"
  | "dismissedNextUp"
  | "continueWatchingShowResumePromptOnLaunch"
  | "continueWatchingSortMode"
>;

export const CONTINUE_WATCHING_DEFAULTS: ContinueWatchingPreferences = {
  continueWatchingVisible: true,
  continueWatchingStyle: "Card",
  continueWatchingUpNextFromFurthestEpisode: true,
  continueWatchingUseEpisodeThumbnails: true,
  continueWatchingShowUnairedNextUp: true,
  continueWatchingBlurNextUp: false,
  dismissedNextUp: [],
  continueWatchingShowResumePromptOnLaunch: true,
  continueWatchingSortMode: "DEFAULT",
};

export type ContinueWatchingCard = {
  item: ContentMeta;
  video?: Video;
  progress?: ResumePoint;
  nextUp: boolean;
  lastWatched: number;
  /** The completed episode that produced this Next Up suggestion. */
  seedSeason?: number;
  seedEpisode?: number;
};

export function continueWatchingPreferences(
  settings?: Partial<ContinueWatchingPreferences> | null,
): ContinueWatchingPreferences {
  return { ...CONTINUE_WATCHING_DEFAULTS, ...settings };
}

/** Titles worth resolving for Continue Watching metadata, newest first. */
export function continueWatchingCandidates(
  snapshot: ProgressSnapshot,
): Array<{ id: string; type: string; at: number }> {
  const best = new Map<string, { id: string; type: string; at: number }>();
  const consider = (
    id: string,
    type: string,
    at: number,
    qualifies: boolean,
  ) => {
    if (!id || !type || !qualifies) return;
    const key = `${normalizeContentType(type)}:${id}`;
    const existing = best.get(key);
    if (!existing || at > existing.at) best.set(key, { id, type, at });
  };
  for (const entry of snapshot.entries) {
    const partWatched = progressPercent(entry) > 0 && !isCompleted(entry);
    consider(
      entry.contentId,
      entry.contentType,
      entry.lastWatched,
      partWatched || isSeries(entry.contentType),
    );
  }
  for (const item of snapshot.watchedItems)
    consider(
      item.contentId,
      item.contentType,
      item.watchedAt,
      isSeries(item.contentType),
    );
  return [...best.values()].sort((left, right) => right.at - left.at);
}

export function buildContinueWatching(
  snapshot: ProgressSnapshot,
  metadata: ContentMeta[],
  rawSettings?: Partial<ContinueWatchingPreferences> | null,
): ContinueWatchingCard[] {
  const settings = continueWatchingPreferences(rawSettings);
  if (!settings.continueWatchingVisible) return [];
  // Later entries are more authoritative. Home passes catalog rows first,
  // followed by saved library rows and finally canonical resolved metadata.
  const metaByKey = new Map(
    metadata.map((item) => [
      `${normalizeContentType(item.contentType)}:${item.id}`,
      item,
    ]),
  );
  const metaById = new Map(metadata.map((item) => [item.id, item]));
  const contentIds = new Set([
    ...snapshot.entries.map((entry) => entry.contentId),
    ...snapshot.watchedItems.map((item) => item.contentId),
  ]);
  const cards: ContinueWatchingCard[] = [];
  for (const contentId of contentIds) {
    const progressType =
      snapshot.entries.find((entry) => entry.contentId === contentId)
        ?.contentType ??
      snapshot.watchedItems.find((entry) => entry.contentId === contentId)
        ?.contentType ??
      "";
    const item =
      metaByKey.get(`${normalizeContentType(progressType)}:${contentId}`) ??
      metaById.get(contentId);
    if (!item) continue;
    const entries = entriesFor(snapshot, contentId);
    const resumable = entries.find(
      (entry) => progressPercent(entry) > 0 && !isCompleted(entry),
    );
    if (resumable) {
      cards.push({
        item,
        video: findVideo(item, resumable),
        progress: resumable,
        nextUp: false,
        lastWatched: resumable.lastWatched,
      });
      continue;
    }
    if (!isSeries(item.contentType)) continue;
    const completed = completedEpisodeSeed(
      snapshot,
      item,
      entries,
      settings.continueWatchingUpNextFromFurthestEpisode,
    );
    if (!completed) continue;
    const next = nextVideo(
      item.videos,
      completed.season,
      completed.episode,
      settings.continueWatchingShowUnairedNextUp,
    );
    if (!next) continue;
    // Official Nuvio keys a dismissal by the completed seed episode, not the
    // episode being suggested.
    const dismissKey = `${item.id}|${completed.season}|${completed.episode}`;
    if (settings.dismissedNextUp.includes(dismissKey)) continue;
    cards.push({
      item,
      video: next,
      nextUp: true,
      lastWatched: completed.lastWatched,
      seedSeason: completed.season,
      seedEpisode: completed.episode,
    });
  }

  const recent = cards.sort(
    (left, right) => right.lastWatched - left.lastWatched,
  );
  if (settings.continueWatchingSortMode !== "STREAMING_STYLE")
    return recent.slice(0, 40);
  const released = recent.filter((card) => !isFutureNextUp(card));
  const upcoming = recent
    .filter(isFutureNextUp)
    .sort((left, right) => releaseTime(left) - releaseTime(right));
  return [...released, ...upcoming].slice(0, 40);
}

export function splitContinueWatching(
  cards: ContinueWatchingCard[],
  mode: ContinueWatchingPreferences["continueWatchingSortMode"],
): { current: ContinueWatchingCard[]; upcoming: ContinueWatchingCard[] } {
  if (mode !== "SPLIT_UPCOMING") return { current: cards, upcoming: [] };
  return {
    current: cards.filter((card) => !isFutureNextUp(card)),
    upcoming: cards
      .filter(isFutureNextUp)
      .sort((left, right) => releaseTime(left) - releaseTime(right)),
  };
}

export function artworkForContinueWatching(
  card: ContinueWatchingCard,
  rawSettings?: Partial<ContinueWatchingPreferences> | null,
) {
  const settings = continueWatchingPreferences(rawSettings);
  if (settings.continueWatchingStyle === "Poster")
    return (
      card.item.poster ||
      card.item.background ||
      card.item.banner ||
      card.video?.thumbnail
    );
  if (settings.continueWatchingUseEpisodeThumbnails)
    return (
      card.video?.thumbnail ||
      card.item.background ||
      card.item.banner ||
      card.item.poster
    );
  return (
    card.item.background ||
    card.item.banner ||
    card.item.poster ||
    card.video?.thumbnail
  );
}

export function remainingLabel(progress?: ResumePoint) {
  if (!progress || progress.durationMs <= 0) return "Continue";
  const minutes = Math.max(
    1,
    Math.ceil((progress.durationMs - progress.positionMs) / 60000),
  );
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return hours > 0
    ? `${hours}h${rest > 0 ? ` ${rest}m` : ""} left`
    : `${minutes}m left`;
}

export function progressForCard(card: ContinueWatchingCard) {
  return card.progress ? progressPercent(card.progress) : 0;
}

export function isFutureNextUp(card: ContinueWatchingCard) {
  return card.nextUp && Number.isFinite(releaseTime(card)) && releaseTime(card) > Date.now();
}

function releaseTime(card: ContinueWatchingCard) {
  return new Date(card.video?.released || "").getTime();
}

function entriesFor(snapshot: ProgressSnapshot, id: string) {
  return snapshot.entries
    .filter((entry) => entry.contentId === id)
    .sort((left, right) => right.lastWatched - left.lastWatched);
}

function normalizeContentType(type: string) {
  switch (type.trim().toLowerCase()) {
    case "tv":
    case "show":
    case "tvshow":
    case "anime":
      return "series";
    case "film":
      return "movie";
    default:
      return type.trim().toLowerCase();
  }
}

function progressPercent(entry: ResumePoint) {
  return entry.durationMs > 0
    ? Math.max(0, Math.min(100, (entry.positionMs / entry.durationMs) * 100))
    : 0;
}

function isCompleted(entry: ResumePoint) {
  return progressPercent(entry) >= 90;
}

function isSeries(type: string) {
  return ["series", "show", "tv", "tvshow", "anime"].includes(
    type.toLowerCase(),
  );
}

function findVideo(item: ContentMeta, entry: ResumePoint) {
  return (
    item.videos.find((video) => video.id === entry.videoId) ||
    item.videos.find(
      (video) =>
        video.season === entry.season && video.episode === entry.episode,
    )
  );
}

function completedEpisodeSeed(
  snapshot: ProgressSnapshot,
  item: ContentMeta,
  entries: ResumePoint[],
  preferFurthestEpisode: boolean,
) {
  const candidates = [
    ...entries
      .filter(
        (entry) =>
          entry.season != null && entry.episode != null && isCompleted(entry),
      )
      .map((entry) => ({
        season: entry.season!,
        episode: entry.episode!,
        lastWatched: entry.lastWatched,
      })),
    ...snapshot.watchedItems
      .filter(
        (watched) =>
          watched.contentId === item.id &&
          watched.season != null &&
          watched.episode != null,
      )
      .map((watched) => ({
        season: watched.season!,
        episode: watched.episode!,
        lastWatched: watched.watchedAt,
      })),
  ];
  return candidates.sort((left, right) =>
    preferFurthestEpisode
      ? right.season - left.season ||
        right.episode - left.episode ||
        right.lastWatched - left.lastWatched
      : right.lastWatched - left.lastWatched ||
        right.season - left.season ||
        right.episode - left.episode,
  )[0];
}

function nextVideo(
  videos: Video[],
  season: number,
  episode: number,
  showUnairedNextUp: boolean,
) {
  const now = Date.now();
  return [...videos]
    .filter((video) => {
      if ((video.season ?? 0) <= 0 || video.available === false) return false;
      if (showUnairedNextUp || !video.released) return true;
      const release = new Date(video.released).getTime();
      return !Number.isFinite(release) || release <= now;
    })
    .sort(
      (left, right) =>
        (left.season ?? 0) - (right.season ?? 0) ||
        (left.episode ?? 0) - (right.episode ?? 0),
    )
    .find(
      (video) =>
        (video.season ?? 0) > season ||
        ((video.season ?? 0) === season && (video.episode ?? 0) > episode),
    );
}

import type { ContentMeta, Video } from "../bridge/types";
import { getValue, setValue } from "./idb";

const KEY = "nuvio.calendar.metas";
const VERSION = 1;
const MAX_AGE_MS = 6 * 60 * 60 * 1000;

type StoredVideo = Pick<
  Video,
  "id" | "title" | "season" | "episode" | "released" | "thumbnail"
>;

type StoredMeta = Pick<
  ContentMeta,
  | "id"
  | "contentType"
  | "name"
  | "poster"
  | "posterShape"
  | "background"
  | "logo"
  | "released"
  | "releaseInfo"
  | "sourceManifestUrl"
  | "addonName"
> & { videos: StoredVideo[] };

type StoredCalendar = {
  version: number;
  savedAt: number;
  scope: string;
  metas: StoredMeta[];
};

function trim(meta: ContentMeta): StoredMeta {
  return {
    id: meta.id,
    contentType: meta.contentType,
    name: meta.name,
    poster: meta.poster,
    posterShape: meta.posterShape,
    background: meta.background,
    logo: meta.logo,
    released: meta.released,
    releaseInfo: meta.releaseInfo,
    sourceManifestUrl: meta.sourceManifestUrl,
    addonName: meta.addonName,
    videos: meta.videos.map((video) => ({
      id: video.id,
      title: video.title,
      season: video.season,
      episode: video.episode,
      released: video.released,
      thumbnail: video.thumbnail,
    })),
  };
}

function restore(stored: StoredMeta): ContentMeta {
  return {
    ...stored,
    genres: [],
    cast: [],
    director: [],
    writer: [],
    trailers: [],
    externalRatings: [],
    hasScheduledVideos: stored.videos.some((video) => !!video.released),
    videos: stored.videos.map((video) => ({ ...video, available: true })),
  };
}

export type CachedCalendar = {
  metas: ContentMeta[];
  stale: boolean;
};

export async function readCalendarMetas(
  scope: string,
): Promise<CachedCalendar | null> {
  try {
    const stored = await getValue<StoredCalendar>(KEY);
    if (!stored || stored.version !== VERSION || stored.scope !== scope)
      return null;
    if (!Array.isArray(stored.metas) || !stored.metas.length) return null;
    return {
      metas: stored.metas.map(restore),
      stale: Date.now() - stored.savedAt > MAX_AGE_MS,
    };
  } catch {
    return null;
  }
}

export async function writeCalendarMetas(
  scope: string,
  metas: ContentMeta[],
): Promise<void> {
  try {
    await setValue<StoredCalendar>(KEY, {
      version: VERSION,
      savedAt: Date.now(),
      scope,
      metas: metas.map(trim),
    });
  } catch {
    // The calendar remains functional if WebView storage is unavailable; it
    // simply has to resolve metadata again the next time it is opened.
  }
}

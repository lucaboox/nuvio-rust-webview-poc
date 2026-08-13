import type { StreamSource } from "../bridge/types";

/**
 * "Reuse last stream" — a device-local cache of the last link played for a
 * given episode, mirroring Nuvio's `StreamLinkCacheRepository`.
 *
 * Deliberately not synced: a resolved playback URL is often tied to the device
 * or session that requested it, so handing it to another client is useless at
 * best. Nuvio keeps this in platform storage for the same reason.
 */
export type CachedStreamLink = {
  url: string;
  streamName: string;
  addonName: string;
  addonId: string;
  cachedAtMs: number;
  requestHeaders?: Record<string, string>;
  filename?: string;
  videoSize?: number;
  infoHash?: string;
  fileIdx?: number;
  sources?: string[];
  bingeGroup?: string;
};

export const DEFAULT_REUSE_CACHE_HOURS = 24;
const STORAGE_PREFIX = "nuvio.streamLink.";

/**
 * Query keys that mark a URL as carrying short-lived playback credentials.
 * Copied from Nuvio's `PlaybackUrlCredentials` — caching a signed debrid link
 * only to have it 403 later is worse than just re-resolving it.
 */
const CREDENTIAL_KEYS = new Set([
  "accesskey", "accesssignature", "accesssig", "access_token", "accesstoken",
  "auth", "authkey", "authsig", "authsignature", "auth_token", "authtoken",
  "e", "exp", "expiration", "expire", "expires", "expiresat", "expiresin",
  "expires_in", "expiry", "hmac", "jwt", "keypairid", "policy", "sig",
  "signature", "signed", "st", "t", "token",
]);
const CREDENTIAL_FRAGMENTS = ["token", "signature", "expires", "expiry"];

export function hasExpiringCredentials(url: string): boolean {
  const query = url.split("?").slice(1).join("?").split("#")[0];
  if (!query.trim()) return false;
  return query.split(/[&;]/).some((parameter) => {
    const rawKey = parameter.split("=")[0].trim().toLowerCase();
    if (!rawKey) return false;
    const compact = rawKey.replace(/[-_.]/g, "");
    return (
      CREDENTIAL_KEYS.has(rawKey) ||
      CREDENTIAL_KEYS.has(compact) ||
      CREDENTIAL_FRAGMENTS.some(
        (fragment) => rawKey.includes(fragment) || compact.includes(fragment),
      )
    );
  });
}

/** Mirrors `StreamLinkCacheRepository.contentKey`. */
export function contentKey(
  type: string,
  videoId: string,
  parentMetaId?: string,
  season?: number,
  episode?: number,
): string {
  const normalized = type.toLowerCase();
  return parentMetaId?.trim() && season != null && episode != null
    ? `${normalized}|${parentMetaId.trim()}|s${season}|e${episode}|${videoId}`
    : `${normalized}|${videoId}`;
}

function storageKey(key: string) {
  return STORAGE_PREFIX + key;
}

export function saveStreamLink(key: string, stream: StreamSource) {
  const url = stream.url ?? "";
  // Nuvio drops rather than stores a credentialed URL, and clears any previous
  // entry so a stale one cannot be served in its place.
  if (url && hasExpiringCredentials(url)) {
    removeStreamLink(key);
    return;
  }
  if (!url && !stream.infoHash) return;

  const entry: CachedStreamLink = {
    url,
    streamName: stream.name || stream.title || "Last used source",
    addonName: stream.addonName,
    addonId: stream.addonId,
    cachedAtMs: Date.now(),
    requestHeaders: stream.behaviorHints?.proxyHeaders?.request,
    filename: stream.behaviorHints?.filename,
    videoSize: stream.behaviorHints?.videoSize,
    infoHash: stream.infoHash,
    fileIdx: stream.fileIdx,
    sources: stream.sources,
    bingeGroup: stream.behaviorHints?.bingeGroup,
  };
  try {
    localStorage.setItem(storageKey(key), JSON.stringify(entry));
  } catch {
    // A full store must not break playback.
  }
}

export function removeStreamLink(key: string) {
  try {
    localStorage.removeItem(storageKey(key));
  } catch {
    // ignored — see saveStreamLink
  }
}

/** Mirrors `getValid`: expired, credentialed and unusable entries are evicted. */
export function getValidStreamLink(
  key: string,
  maxAgeMs: number,
): CachedStreamLink | null {
  if (maxAgeMs <= 0) return null;
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(storageKey(key));
  } catch {
    return null;
  }
  if (!raw) return null;

  let entry: CachedStreamLink;
  try {
    entry = JSON.parse(raw) as CachedStreamLink;
  } catch {
    removeStreamLink(key);
    return null;
  }

  const age = Date.now() - entry.cachedAtMs;
  if (!entry.cachedAtMs || age > maxAgeMs) {
    removeStreamLink(key);
    return null;
  }
  if (entry.url && hasExpiringCredentials(entry.url)) {
    removeStreamLink(key);
    return null;
  }
  if (!entry.url && !entry.infoHash) {
    removeStreamLink(key);
    return null;
  }
  return entry;
}

/** Rebuilds a playable stream from a cache entry. */
export function cachedStreamToSource(entry: CachedStreamLink): StreamSource {
  return {
    name: entry.streamName,
    title: entry.streamName,
    description: "",
    url: entry.url || undefined,
    infoHash: entry.infoHash,
    fileIdx: entry.fileIdx,
    sources: entry.sources ?? [],
    addonName: entry.addonName,
    addonId: entry.addonId,
    behaviorHints: {
      filename: entry.filename,
      videoSize: entry.videoSize,
      bingeGroup: entry.bingeGroup,
      notWebReady: false,
      proxyHeaders: { request: entry.requestHeaders },
    },
  };
}

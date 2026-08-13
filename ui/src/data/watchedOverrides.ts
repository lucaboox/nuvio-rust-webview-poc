import { useSyncExternalStore } from "react";

/**
 * Optimistic watched flips for episodes.
 *
 * The right-click menu and the episode rows are separate components, so the
 * pending value lives here rather than in either one. Each override is held
 * until the refreshed progress snapshot agrees with it — releasing on the RPC
 * response alone would let the row flicker back while the snapshot is still in
 * flight.
 */
const overrides = new Map<string, boolean>();
const listeners = new Set<() => void>();
let version = 0;

export function watchedKey(contentId: string, season?: number, episode?: number) {
  return `${contentId}:${season ?? ""}:${episode ?? ""}`;
}

function notify() {
  version += 1;
  listeners.forEach((listener) => listener());
}

export function setWatchedOverride(key: string, watched: boolean) {
  overrides.set(key, watched);
  notify();
}

export function clearWatchedOverride(key: string) {
  if (overrides.delete(key)) notify();
}

export function getWatchedOverride(key: string): boolean | undefined {
  return overrides.get(key);
}

/** Drops any override the given snapshot has caught up with. */
export function reconcileWatchedOverrides(
  isWatched: (key: string) => boolean,
) {
  let changed = false;
  for (const [key, pending] of [...overrides]) {
    if (isWatched(key) === pending) {
      overrides.delete(key);
      changed = true;
    }
  }
  if (changed) notify();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Re-renders consumers whenever an override is added or released. */
export function useWatchedOverrides() {
  return useSyncExternalStore(
    subscribe,
    () => version,
    () => version,
  );
}

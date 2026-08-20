import { useSyncExternalStore } from "react";

/**
 * Settings that exist only in this client.
 *
 * Nuvio has no equivalent for these, so there is nothing to sync them with —
 * writing them into the profile blob would put keys in it that no other client
 * understands. They live in localStorage and are labelled as device-only in the
 * settings UI.
 */
export type ClientSettings = {
  /** Left click on the video toggles pause. */
  clickToPause: boolean;
  /** Decode a preview frame when hovering the seek bar. */
  seekThumbnails: boolean;
  /** Allow submitting marked intro timings to IntroDB. Official Nuvio keeps
   * this device-local and deliberately omits it from profile sync. */
  introSubmitEnabled: boolean;
};

const DEFAULTS: ClientSettings = {
  clickToPause: true,
  seekThumbnails: false,
  introSubmitEnabled: false,
};

const STORAGE_KEY = "nuvio.clientSettings";
const listeners = new Set<() => void>();

function read(): ClientSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULTS;
    const stored = JSON.parse(raw) as Partial<ClientSettings>;
    return {
      clickToPause: stored.clickToPause ?? DEFAULTS.clickToPause,
      seekThumbnails: stored.seekThumbnails ?? DEFAULTS.seekThumbnails,
      introSubmitEnabled:
        stored.introSubmitEnabled ?? DEFAULTS.introSubmitEnabled,
    };
  } catch {
    return DEFAULTS;
  }
}

// useSyncExternalStore compares by reference, so the snapshot is only replaced
// when something actually changes.
let snapshot: ClientSettings = read();

export function setClientSetting<K extends keyof ClientSettings>(
  key: K,
  value: ClientSettings[K],
) {
  const next = { ...snapshot, [key]: value };
  if (next[key] === snapshot[key]) return;
  snapshot = next;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Applies for this session even if the store is unavailable.
  }
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useClientSettings(): ClientSettings {
  return useSyncExternalStore(
    subscribe,
    () => snapshot,
    () => snapshot,
  );
}

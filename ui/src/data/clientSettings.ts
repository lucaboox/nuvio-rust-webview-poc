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
  /** Hold right click for temporary fast-forward. */
  holdToSpeed: boolean;
  holdSpeed: number;
  /** Decode a preview frame when hovering the seek bar. */
  seekThumbnails: boolean;
};

const DEFAULTS: ClientSettings = {
  clickToPause: true,
  holdToSpeed: true,
  holdSpeed: 2,
  seekThumbnails: false,
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
      holdToSpeed: stored.holdToSpeed ?? DEFAULTS.holdToSpeed,
      holdSpeed: clampSpeed(stored.holdSpeed),
      seekThumbnails: stored.seekThumbnails ?? DEFAULTS.seekThumbnails,
    };
  } catch {
    return DEFAULTS;
  }
}

function clampSpeed(value: unknown): number {
  const speed = Number(value);
  if (!Number.isFinite(speed)) return DEFAULTS.holdSpeed;
  return Math.min(Math.max(speed, 1.25), 4);
}

// useSyncExternalStore compares by reference, so the snapshot is only replaced
// when something actually changes.
let snapshot: ClientSettings = read();

export function setClientSetting<K extends keyof ClientSettings>(
  key: K,
  value: ClientSettings[K],
) {
  const next = { ...snapshot, [key]: value };
  if (key === "holdSpeed") next.holdSpeed = clampSpeed(value);
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

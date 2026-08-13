import { useSyncExternalStore } from "react";
import type { ContentMeta, LibraryItem } from "../bridge/types";

/**
 * In-memory membership set for the library.
 *
 * The context menu used to call `library.list` on every right-click, which is a
 * Supabase round trip — hence the visible delay before "Add"/"Remove" settled.
 * Nuvio keeps the library in a repository and reads it synchronously; this is
 * the same idea. The list is primed once per profile and kept current by the
 * add/remove calls themselves.
 */
const membership = new Set<string>();
const listeners = new Set<() => void>();
let version = 0;
let primed = false;

function key(item: Pick<ContentMeta, "id" | "contentType">) {
  return `${item.contentType}:${item.id}`;
}

function notify() {
  version += 1;
  listeners.forEach((listener) => listener());
}

export function primeLibrary(items: LibraryItem[]) {
  membership.clear();
  for (const item of items) membership.add(key(item));
  primed = true;
  notify();
}

export function resetLibraryCache() {
  membership.clear();
  primed = false;
  notify();
}

export function isLibraryPrimed() {
  return primed;
}

export function isInLibrary(item: Pick<ContentMeta, "id" | "contentType">) {
  return membership.has(key(item));
}

export function setLibraryMembership(
  item: Pick<ContentMeta, "id" | "contentType">,
  saved: boolean,
) {
  const entry = key(item);
  if (saved === membership.has(entry)) return;
  if (saved) membership.add(entry);
  else membership.delete(entry);
  notify();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Re-renders only when membership actually changes. */
export function useInLibrary(
  item: Pick<ContentMeta, "id" | "contentType"> | null,
) {
  const snapshot = useSyncExternalStore(
    subscribe,
    () => version,
    () => version,
  );
  void snapshot;
  return item ? membership.has(key(item)) : false;
}

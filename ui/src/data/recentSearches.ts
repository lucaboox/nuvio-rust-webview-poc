const STORAGE_KEY = "nuvio.recentSearches";
const LIMIT = 10;

/** Device-local search history — never synced, and capped at ten entries. */
export function readRecentSearches(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed.filter((entry): entry is string => typeof entry === "string").slice(0, LIMIT)
      : [];
  } catch {
    return [];
  }
}

export function rememberSearch(query: string): string[] {
  const value = query.trim();
  if (!value) return readRecentSearches();
  // Case-insensitive dedupe so "dune" and "Dune" do not both occupy a slot.
  const next = [
    value,
    ...readRecentSearches().filter(
      (entry) => entry.toLowerCase() !== value.toLowerCase(),
    ),
  ].slice(0, LIMIT);
  write(next);
  return next;
}

export function forgetSearch(query: string): string[] {
  const next = readRecentSearches().filter((entry) => entry !== query);
  write(next);
  return next;
}

export function clearRecentSearches(): string[] {
  write([]);
  return [];
}

function write(entries: string[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(entries));
  } catch {
    // History is a convenience; a full store must not break search.
  }
}

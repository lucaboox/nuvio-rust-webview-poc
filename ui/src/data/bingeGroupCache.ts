const STORAGE_PREFIX = "nuvio.bingeGroup.";

export function saveBingeGroup(contentId: string, bingeGroup?: string) {
  const value = bingeGroup?.trim();
  if (!contentId.trim() || !value) return;
  try {
    localStorage.setItem(STORAGE_PREFIX + contentId.trim(), value);
  } catch {
    // Device-local continuity is optional and must never block playback.
  }
}

export function getBingeGroup(contentId: string): string | null {
  if (!contentId.trim()) return null;
  try {
    return localStorage.getItem(STORAGE_PREFIX + contentId.trim());
  } catch {
    return null;
  }
}

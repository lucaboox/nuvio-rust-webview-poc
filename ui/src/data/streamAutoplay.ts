import type { SettingsSnapshot, StreamSource } from "../bridge/types";

/**
 * The Rust prototype currently receives Stremio addon streams as one stable,
 * addon-ordered batch. These helpers mirror Nuvio's selection policy over that
 * batch: a matching binge group wins first, then the configured mode is used.
 */
export function autoplayCandidates(
  streams: StreamSource[],
  settings?: SettingsSnapshot | null,
  forNextEpisode = false,
): StreamSource[] {
  const manualNextFlow =
    forNextEpisode &&
    settings?.autoplayMode === "MANUAL" &&
    (!!settings.autoplayNextEpisode || !!settings.autoplayPreferBingeGroup);
  const source = manualNextFlow
    ? "ALL_SOURCES"
    : normalizeAutoplaySource(settings?.autoplaySource);

  // This client only exposes installed Stremio addons today. Preserve Nuvio's
  // source-scope behavior so a synced "plugins only" value cannot silently pick
  // an addon stream.
  if (source === "ENABLED_PLUGINS_ONLY") return [];
  const selectedAddons = manualNextFlow
    ? []
    : (settings?.autoplaySelectedAddons ?? []);
  return streams.filter(
    (stream) =>
      isAutoPlayable(stream) &&
      (selectedAddons.length === 0 || selectedAddons.includes(stream.addonName)),
  );
}

export function selectPreferredBingeGroup(
  candidates: StreamSource[],
  bingeGroup?: string | null,
): StreamSource | null {
  const target = bingeGroup?.trim();
  if (!target) return null;
  return (
    candidates.find(
      (stream) => stream.behaviorHints?.bingeGroup?.trim() === target,
    ) ?? null
  );
}

export function selectAutoplayFallback(
  candidates: StreamSource[],
  settings?: SettingsSnapshot | null,
  forNextEpisode = false,
): StreamSource | null {
  const mode = settings?.autoplayMode ?? "MANUAL";
  if (mode === "MANUAL") {
    if (!forNextEpisode || !settings?.autoplayNextEpisode) return null;
    // Nuvio treats Manual + preferred group + disabled fallback as
    // binge-group-only. If the preferred group was absent, show the picker.
    if (
      settings.autoplayPreferBingeGroup &&
      !settings.autoplayNextEpisodeFallback
    ) {
      return null;
    }
    return candidates[0] ?? null;
  }
  if (mode === "FIRST_STREAM") return candidates[0] ?? null;

  const pattern = settings?.autoplayRegex?.trim();
  if (!pattern) return null;
  let regex: RegExp;
  try {
    regex = new RegExp(pattern, "i");
  } catch {
    return null;
  }
  return candidates.find((stream) => regex.test(streamSearchText(stream))) ?? null;
}

/**
 * Initial playback never inherits the previous episode's binge group. That
 * group is only an input to PlayerPage's next-episode transition. In Manual
 * mode this must therefore return null even when both binge-group preferences
 * are enabled and a matching source exists.
 */
export function selectInitialAutoplay(
  candidates: StreamSource[],
  settings?: SettingsSnapshot | null,
): StreamSource | null {
  return selectAutoplayFallback(candidates, settings, false);
}

/** Wait only for the unspent part of Nuvio's selection window. */
export async function waitForAutoplayWindow(
  startedAt: number,
  seconds?: number,
): Promise<void> {
  const duration = Math.max(0, Math.min(seconds ?? 0, 30)) * 1000;
  const remaining = duration - (performance.now() - startedAt);
  if (remaining > 0) {
    await new Promise<void>((resolve) => window.setTimeout(resolve, remaining));
  }
}

function isAutoPlayable(stream: StreamSource): boolean {
  return !!stream.url || !!stream.externalUrl;
}

function normalizeAutoplaySource(value?: string) {
  switch (value) {
    case "ADDONS_ONLY":
    case "INSTALLED_ADDONS_ONLY":
      return "INSTALLED_ADDONS_ONLY";
    case "PLUGINS_ONLY":
    case "ENABLED_PLUGINS_ONLY":
      return "ENABLED_PLUGINS_ONLY";
    default:
      return "ALL_SOURCES";
  }
}

function streamSearchText(stream: StreamSource): string {
  return [
    stream.addonName,
    stream.name,
    stream.title,
    stream.description,
    stream.url,
    stream.externalUrl,
  ]
    .filter(Boolean)
    .join(" ");
}

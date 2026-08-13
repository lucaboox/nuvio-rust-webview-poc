import type { CSSProperties } from "react";

/**
 * Poster card style, mirroring Nuvio's `PosterCardStyleRepository`.
 *
 * This is NOT device-local: Nuvio persists it in the profile settings blob, and
 * this client shares that blob's `desktop` platform row. It is one global style
 * rather than per-surface — home, search and catalog all read the same width.
 */
export const POSTER_WIDTHS = [
  ["Compact", 104],
  ["Dense", 112],
  ["Standard", 120],
  ["Balanced", 126],
  ["Comfort", 134],
  ["Large", 140],
] as const;

export const POSTER_RADII = [
  ["Sharp", 0],
  ["Subtle", 4],
  ["Classic", 8],
  ["Rounded", 12],
  ["Pill", 16],
] as const;

export const DEFAULT_POSTER_WIDTH = 126;
export const DEFAULT_POSTER_CORNER_RADIUS = 12;

export type PosterCardStyle = {
  width: number;
  cornerRadius: number;
  hideLabels: boolean;
  landscapeCatalogs: boolean;
};

/**
 * Nuvio's widths are density-independent pixels sized for phones; the desktop
 * client renders the same cards on a much larger canvas, so they are scaled up
 * to stay proportionate while still tracking the synced preset.
 */
const DESKTOP_SCALE = 1.22;

export function posterCardStyle(settings: {
  posterWidth?: number;
  posterCornerRadius?: number;
  posterHideLabels?: boolean;
  posterLandscapeCatalogs?: boolean;
} | null): PosterCardStyle {
  return {
    width: settings?.posterWidth ?? DEFAULT_POSTER_WIDTH,
    cornerRadius: settings?.posterCornerRadius ?? DEFAULT_POSTER_CORNER_RADIUS,
    hideLabels: settings?.posterHideLabels ?? false,
    landscapeCatalogs: settings?.posterLandscapeCatalogs ?? false,
  };
}

/** Custom properties that scope poster sizing for a subtree. */
export function posterStyleVars(style: PosterCardStyle): CSSProperties {
  return {
    "--poster-w": `${Math.round(style.width * DESKTOP_SCALE)}px`,
    "--poster-radius": `${style.cornerRadius}px`,
  } as CSSProperties;
}

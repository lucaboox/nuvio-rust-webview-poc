import {
  CalendarDays, Compass, Download, House, Layers, Library, ListVideo, Pause, Play, Puzzle,
  Settings, SlidersHorizontal, type LucideIcon,
} from "lucide-react";

type IconName = "home" | "discover" | "library" | "calendar" | "downloads" | "collections" | "addons" | "settings" | "search" | "play" | "pause" | "rewind" | "forward" | "volume" | "muted" | "fullscreen" | "subtitles" | "audio" | "info" | "logout" | "plus" | "back" | "close" | "refresh" | "copy" | "up" | "down" | "edit" | "drag" | "trash" | "external" | "gear" | "check" | "video" | "episodes" | "sources" | "eye";

/**
 * Icons sourced from Lucide, whose 24x24 / round-cap style this set already
 * imitates. Hand-drawn geometry is kept for the rest; anything added here
 * renders at the same stroke weight so the two blend.
 */
const lucide: Partial<Record<IconName, LucideIcon>> = {
  episodes: ListVideo,
  sources: Layers,
  home: House,
  discover: Compass,
  library: Library,
  calendar: CalendarDays,
  downloads: Download,
  addons: Puzzle,
  settings: SlidersHorizontal,
  // Distinct from `settings`, which is the sliders glyph used for the nav. A
  // cog is what an addon's own configuration page is labelled with elsewhere.
  gear: Settings,
  play: Play,
  pause: Pause,
};

const paths: Record<IconName, React.ReactNode> = {
  home: <><path d="m3 10 9-7 9 7"/><path d="M5 9v11h14V9"/><path d="M9 20v-6h6v6"/></>,
  discover: <><circle cx="12" cy="12" r="9"/><path d="m16 8-2.4 5.6L8 16l2.4-5.6L16 8Z"/><circle cx="12" cy="12" r=".8" fill="currentColor" stroke="none"/></>,
  library: <><path d="M4 4.5v15"/><path d="M8.5 6.5v13"/><path d="M13 4.5v15"/><path d="m16.9 5.6 3.9 13.6"/></>,
  calendar: <><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M16 3v4M8 3v4M3 10h18"/></>,
  downloads: <><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 20h14"/></>,
  collections: <><rect x="3.5" y="4" width="7" height="7" rx="1.5"/><rect x="13.5" y="4" width="7" height="7" rx="1.5"/><rect x="3.5" y="14" width="7" height="6" rx="1.5"/><rect x="13.5" y="14" width="7" height="6" rx="1.5"/></>,
  addons: <path d="M6.5 6h3.3a2.2 2.2 0 1 1 4.4 0h3.3a1 1 0 0 1 1 1v3.3a2.2 2.2 0 1 0 0 4.4V18a1 1 0 0 1-1 1H6.5a1 1 0 0 1-1-1v-3.3a2.2 2.2 0 1 0 0-4.4V7a1 1 0 0 1 1-1Z"/>,
  settings: <><path d="M4 7h8"/><path d="M16.5 7H20"/><path d="M4 17h3.5"/><path d="M12 17h8"/><circle cx="14.2" cy="7" r="2.3"/><circle cx="9.8" cy="17" r="2.3"/></>,
  search: <><circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 4.5 4.5"/></>,
  play: <path d="m9 6 9 6-9 6V6Z" fill="currentColor" stroke="none"/>,
  pause: <><path d="M8 6v12M16 6v12" strokeWidth="3"/></>,
  rewind: <><path d="m11 8-4 4 4 4"/><path d="M7 12h8a4 4 0 0 1 4 4"/></>,
  forward: <><path d="m13 8 4 4-4 4"/><path d="M17 12H9a4 4 0 0 0-4 4"/></>,
  volume: <><path d="M5 10v4h4l5 4V6L9 10H5Z"/><path d="M17 9a4 4 0 0 1 0 6M19 6.5a8 8 0 0 1 0 11"/></>,
  muted: <><path d="M5 10v4h4l5 4V6L9 10H5Z"/><path d="m17 10 4 4M21 10l-4 4"/></>,
  fullscreen: <><path d="M8 4H4v4M16 4h4v4M8 20H4v-4M16 20h4v-4"/></>,
  subtitles: <><rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 12h4M7 15h6M14 12h3M15 15h2"/></>,
  audio: <><path d="M9 18V6l10-2v12"/><circle cx="6" cy="18" r="3"/><circle cx="16" cy="16" r="3"/></>,
  info: <><circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.2"/></>,
  logout: <><path d="M10 5H5v14h5"/><path d="M13 8l4 4-4 4M8 12h9"/></>,
  plus: <><path d="M12 5v14M5 12h14"/></>,
  back: <><path d="m14 5-7 7 7 7"/><path d="M7 12h11"/></>,
  close: <><path d="m7 7 10 10M17 7 7 17"/></>,
  refresh: <><path d="M20 12a8 8 0 1 1-2.34-5.66"/><path d="M20 4v5h-5"/></>,
  copy: <><rect x="8" y="8" width="11" height="11" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/></>,
  up: <path d="m6 15 6-6 6 6"/>,
  down: <path d="m6 9 6 6 6-6"/>,
  edit: <><path d="M4 20h4l10-10a2.1 2.1 0 0 0-3-3L5 17v3Z"/><path d="m14.5 5.5 3 3"/></>,
  drag: <><path d="M4 8h16M4 12h16M4 16h16"/></>,
  trash: <><path d="M4 7h16"/><path d="M10 4h4a1 1 0 0 1 1 1v2H9V5a1 1 0 0 1 1-1Z"/><path d="M6.5 7v12.5a1.5 1.5 0 0 0 1.5 1.5h8a1.5 1.5 0 0 0 1.5-1.5V7"/><path d="M10.5 11v6M13.5 11v6"/></>,
  external: <><path d="M14 4h6v6"/><path d="m20 4-8.5 8.5"/><path d="M18 14.5V19a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 4 19V8a1.5 1.5 0 0 1 1.5-1.5H10"/></>,
  gear: <><circle cx="12" cy="12" r="3.2"/><path d="M19.4 15a1.6 1.6 0 0 0 .32 1.77l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.6 1.6 0 0 0-1.77-.32 1.6 1.6 0 0 0-1 1.47V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-1.05-1.47 1.6 1.6 0 0 0-1.77.32l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.6 1.6 0 0 0 .32-1.77 1.6 1.6 0 0 0-1.47-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.47-1.05 1.6 1.6 0 0 0-.32-1.77l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.6 1.6 0 0 0 1.77.32H9a1.6 1.6 0 0 0 1-1.47V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.47 1.6 1.6 0 0 0 1.77-.32l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.6 1.6 0 0 0-.32 1.77V9a1.6 1.6 0 0 0 1.47 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.47 1Z"/></>,
  check: <path d="m5 12.5 4.5 4.5L19 7.5"/>,
  eye: <><path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12Z"/><circle cx="12" cy="12" r="3.2"/></>,
  episodes: <><rect x="3" y="4.5" width="13" height="10" rx="1.6"/><path d="M18.5 6.5v11.5a1.5 1.5 0 0 1-1.5 1.5H6"/><path d="M21 8.5v9"/></>,
  sources: <><path d="M12 3.2 20.5 8 12 12.8 3.5 8Z"/><path d="m3.5 12 8.5 4.8 8.5-4.8"/><path d="m3.5 16 8.5 4.8 8.5-4.8"/></>,
  video: <><rect x="2.5" y="4.5" width="19" height="15" rx="2.5"/><path d="M10 9.2v5.6l4.8-2.8z"/></>,
};

export function Icon({ name, size = 21 }: { name: IconName; size?: number }) {
  const Glyph = lucide[name];
  if (Glyph) return <Glyph size={size} strokeWidth={1.7} aria-hidden="true" />;
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}

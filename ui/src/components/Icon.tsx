type IconName = "home" | "discover" | "library" | "collections" | "addons" | "settings" | "search" | "play" | "pause" | "rewind" | "forward" | "volume" | "muted" | "fullscreen" | "subtitles" | "audio" | "info" | "logout" | "plus" | "back" | "close" | "refresh" | "copy" | "up" | "down";

const paths: Record<IconName, React.ReactNode> = {
  home: <><path d="m3 10 9-7 9 7"/><path d="M5 9v11h14V9"/><path d="M9 20v-6h6v6"/></>,
  discover: <><circle cx="12" cy="12" r="9"/><path d="m16 8-2.4 5.6L8 16l2.4-5.6L16 8Z"/><circle cx="12" cy="12" r=".8" fill="currentColor" stroke="none"/></>,
  library: <><rect x="4" y="4" width="16" height="16" rx="2"/><path d="M8 4v16M12 4v16"/></>,
  collections: <><rect x="3.5" y="4" width="7" height="7" rx="1.5"/><rect x="13.5" y="4" width="7" height="7" rx="1.5"/><rect x="3.5" y="14" width="7" height="6" rx="1.5"/><rect x="13.5" y="14" width="7" height="6" rx="1.5"/></>,
  addons: <path d="M19.4 13.5h-1.7a2.7 2.7 0 1 0-5.2 0H9.6v-2.9a2.7 2.7 0 1 0 0-5.2V3.6H4.1a.5.5 0 0 0-.5.5v5.5h1.8a2.7 2.7 0 1 1 0 5.2H3.6v5.1c0 .3.2.5.5.5h5.5v-1.7a2.7 2.7 0 1 1 5.2 0v1.7h5.1c.3 0 .5-.2.5-.5v-5.9c0-.3-.4-.5-1-.5Z"/>,
  settings: <><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V21h-4v-.08A1.7 1.7 0 0 0 8.95 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.58 15 1.7 1.7 0 0 0 3 14H3v-4h.08A1.7 1.7 0 0 0 4.6 8.95a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.58 1.7 1.7 0 0 0 10 3V3h4v.08a1.7 1.7 0 0 0 1.05 1.52 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.42 9 1.7 1.7 0 0 0 21 10v4h-.08A1.7 1.7 0 0 0 19.4 15Z"/></>,
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
};

export function Icon({ name, size = 21 }: { name: IconName; size?: number }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}

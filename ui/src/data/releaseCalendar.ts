import type { ContentMeta, Video } from "../bridge/types";

export type ReleaseCalendarItem = {
  key: string;
  date: string;
  meta: ContentMeta;
  video?: Video;
  kind: "movie" | "episode";
};

/**
 * Nuvio metadata may contain either a calendar-only date or a timestamp. A
 * timestamp is converted to the viewer's local day, matching the official
 * client's notification/release-date policy.
 */
export function localReleaseDate(value?: string): string | null {
  const input = value?.trim();
  if (!input) return null;
  const plain = /^(\d{4})-(\d{2})-(\d{2})$/.exec(input);
  if (plain) {
    const year = Number(plain[1]);
    const month = Number(plain[2]);
    const day = Number(plain[3]);
    const check = new Date(year, month - 1, day);
    return check.getFullYear() === year &&
      check.getMonth() === month - 1 &&
      check.getDate() === day
      ? `${plain[1]}-${plain[2]}-${plain[3]}`
      : null;
  }
  const instant = new Date(input);
  if (Number.isNaN(instant.getTime())) return null;
  return [
    instant.getFullYear().toString().padStart(4, "0"),
    String(instant.getMonth() + 1).padStart(2, "0"),
    String(instant.getDate()).padStart(2, "0"),
  ].join("-");
}

export function buildReleaseCalendar(
  metas: ContentMeta[],
): ReleaseCalendarItem[] {
  const unique = new Map<string, ReleaseCalendarItem>();
  for (const meta of metas) {
    if (meta.contentType.toLowerCase() === "movie") {
      const date = localReleaseDate(meta.released);
      if (!date) continue;
      const item: ReleaseCalendarItem = {
        key: `${date}:movie:${meta.id}`,
        date,
        meta,
        kind: "movie",
      };
      unique.set(item.key, item);
      continue;
    }
    for (const video of meta.videos) {
      const date = localReleaseDate(video.released);
      if (!date) continue;
      const item: ReleaseCalendarItem = {
        key: `${date}:episode:${meta.id}:${video.id}`,
        date,
        meta,
        video,
        kind: "episode",
      };
      unique.set(item.key, item);
    }
  }
  return [...unique.values()].sort(
    (left, right) =>
      left.date.localeCompare(right.date) ||
      (left.video?.season ?? -1) - (right.video?.season ?? -1) ||
      (left.video?.episode ?? -1) - (right.video?.episode ?? -1) ||
      left.meta.name.localeCompare(right.meta.name),
  );
}

export function monthPrefix(year: number, monthIndex: number): string {
  return `${year.toString().padStart(4, "0")}-${String(monthIndex + 1).padStart(2, "0")}`;
}

export function monthCells(
  year: number,
  monthIndex: number,
  mondayFirst = false,
): Array<number | null> {
  const weekday = new Date(year, monthIndex, 1).getDay();
  const leading = mondayFirst ? (weekday + 6) % 7 : weekday;
  const count = new Date(year, monthIndex + 1, 0).getDate();
  const cells: Array<number | null> = [
    ...Array.from({ length: leading }, () => null),
    ...Array.from({ length: count }, (_, index) => index + 1),
  ];
  while (cells.length % 7) cells.push(null);
  return cells;
}

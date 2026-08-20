import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { ContentMeta } from "../bridge/types";
import { readCalendarMetas, writeCalendarMetas } from "../data/calendarCache";
import {
  buildReleaseCalendar,
  localReleaseDate,
  monthCells,
  monthPrefix,
  type ReleaseCalendarItem,
} from "../data/releaseCalendar";

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const monthTitle = new Intl.DateTimeFormat(undefined, {
  month: "long",
  year: "numeric",
});
const dayTitle = new Intl.DateTimeFormat(undefined, {
  weekday: "long",
  month: "long",
  day: "numeric",
});

function todayIso() {
  return localReleaseDate(new Date().toISOString())!;
}

function releaseLabel(item: ReleaseCalendarItem) {
  if (!item.video) return "Movie release";
  const episode = [
    item.video.season == null ? "" : `S${item.video.season}`,
    item.video.episode == null ? "" : `E${item.video.episode}`,
  ].join("");
  return `${episode || "Episode"}${item.video.title ? ` · ${item.video.title}` : ""}`;
}

const RESOLVE_CONCURRENCY = 8;
const PROGRESS_INTERVAL_MS = 250;

async function resolveLibrary(
  seeds: ContentMeta[],
  cache: Map<string, ContentMeta>,
  isCurrent: () => boolean,
  onProgress?: (metas: ContentMeta[]) => void,
  refresh = false,
): Promise<ContentMeta[]> {
  const collect = () =>
    seeds
      .map((seed) => cache.get(`${seed.contentType}:${seed.id}`))
      .filter((meta): meta is ContentMeta => !!meta);
  const unresolved = refresh
    ? seeds
    : seeds.filter((seed) => !cache.has(`${seed.contentType}:${seed.id}`));
  if (!unresolved.length) return collect();

  let cursor = 0;
  let lastPublished = 0;
  await Promise.all(
    Array.from(
      { length: Math.min(RESOLVE_CONCURRENCY, unresolved.length) },
      async () => {
        for (;;) {
          const index = cursor;
          cursor += 1;
          if (index >= unresolved.length || !isCurrent()) return;
          const seed = unresolved[index]!;
          const meta = await invoke<ContentMeta>("content.details", {
            id: seed.id,
            type: seed.contentType,
          }).catch(() => seed);
          // A resolved title is useful to every month. Retain it even if the
          // user changed months while this particular request was in flight.
          cache.set(`${meta.contentType}:${meta.id}`, meta);
          if (!isCurrent()) return;
          const now = Date.now();
          if (onProgress && now - lastPublished >= PROGRESS_INTERVAL_MS) {
            lastPublished = now;
            onProgress(collect());
          }
        }
      },
    ),
  );
  return isCurrent() ? collect() : [];
}

export function CalendarPage({
  items,
  ready,
  scope,
  onSelect,
}: {
  items: ContentMeta[];
  ready: boolean;
  scope: string;
  onSelect(item: ContentMeta): void;
}) {
  const now = new Date();
  const [visibleMonth, setVisibleMonth] = useState(
    () => new Date(now.getFullYear(), now.getMonth(), 1),
  );
  const [monthReleases, setMonthReleases] = useState<ReleaseCalendarItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [hydration, setHydration] = useState<"pending" | "done">("pending");
  const [selectedDate, setSelectedDate] = useState(todayIso);
  const [datasetRevision, setDatasetRevision] = useState(0);
  const [monthDirection, setMonthDirection] = useState<"next" | "previous">("next");
  const metadataCache = useRef(new Map<string, ContentMeta>());
  const monthCache = useRef(new Map<string, ReleaseCalendarItem[]>());
  const loadGeneration = useRef(0);
  const needsRefresh = useRef(true);
  const year = visibleMonth.getFullYear();
  const month = visibleMonth.getMonth();
  const prefix = monthPrefix(year, month);

  const identity = items.map((item) => `${item.contentType}:${item.id}`).join("|");
  useEffect(() => {
    let active = true;
    metadataCache.current.clear();
    monthCache.current.clear();
    loadGeneration.current += 1;
    setMonthReleases([]);
    setHydration("pending");
    void readCalendarMetas(scope).then((cached) => {
      if (!active) return;
      if (cached) {
        for (const meta of cached.metas)
          metadataCache.current.set(`${meta.contentType}:${meta.id}`, meta);
      }
      needsRefresh.current = !cached || cached.stale;
      setHydration("done");
      setDatasetRevision((current) => current + 1);
    });
    return () => {
      active = false;
    };
  }, [identity, ready, scope]);

  useEffect(() => {
    if (hydration !== "done") return;
    const generation = ++loadGeneration.current;
    const cached = monthCache.current.get(prefix);
    if (cached) {
      setMonthReleases(cached);
      setLoading(false);
      return;
    }
    if (!ready || !items.length) {
      setMonthReleases([]);
      setLoading(false);
      return;
    }

    const forThisMonth = (metas: ContentMeta[]) =>
      buildReleaseCalendar(metas).filter((item) =>
        item.date.startsWith(`${prefix}-`),
      );
    const seeded = forThisMonth(
      items
        .map((item) => metadataCache.current.get(`${item.contentType}:${item.id}`))
        .filter((meta): meta is ContentMeta => !!meta),
    );
    setMonthReleases(seeded);

    const complete = items.every((item) =>
      metadataCache.current.has(`${item.contentType}:${item.id}`),
    );
    if (complete && !needsRefresh.current) {
      monthCache.current.set(prefix, seeded);
      setLoading(false);
      return;
    }

    setLoading(!seeded.length);
    void resolveLibrary(
      items,
      metadataCache.current,
      () => generation === loadGeneration.current,
      (metas) => {
        if (generation !== loadGeneration.current) return;
        setMonthReleases(forThisMonth(metas));
      },
      complete && needsRefresh.current,
    )
      .then((metas) => {
        if (generation !== loadGeneration.current) return;
        const releases = forThisMonth(metas);
        monthCache.current.set(prefix, releases);
        setMonthReleases(releases);
        needsRefresh.current = false;
        if (metas.length >= items.length) void writeCalendarMetas(scope, metas);
      })
      .finally(() => {
        if (generation === loadGeneration.current) setLoading(false);
      });
    return () => {
      if (generation === loadGeneration.current) loadGeneration.current += 1;
    };
  }, [prefix, datasetRevision, hydration, ready, items, scope]);

  const releasesByDate = useMemo(() => {
    const grouped = new Map<string, ReleaseCalendarItem[]>();
    for (const item of monthReleases) {
      const current = grouped.get(item.date) ?? [];
      current.push(item);
      grouped.set(item.date, current);
    }
    return grouped;
  }, [monthReleases]);
  const cells = useMemo(() => monthCells(year, month, true), [year, month]);
  const today = todayIso();

  useEffect(() => {
    const currentMonth = today.startsWith(`${prefix}-`);
    const firstRelease = monthReleases[0]?.date;
    setSelectedDate((current) =>
      current.startsWith(`${prefix}-`)
        ? current
        : currentMonth
          ? today
          : firstRelease ?? `${prefix}-01`,
    );
  }, [prefix, today, monthReleases]);

  const selected = releasesByDate.get(selectedDate) ?? [];
  const changeMonth = (offset: number) => {
    setMonthDirection(offset > 0 ? "next" : "previous");
    setVisibleMonth((current) =>
      new Date(current.getFullYear(), current.getMonth() + offset, 1),
    );
  };
  const goToday = () => {
    const current = new Date();
    setVisibleMonth(new Date(current.getFullYear(), current.getMonth(), 1));
    setSelectedDate(todayIso());
  };

  return (
    <div className="calendar-page">
      <header className="calendar-page-title">
        <div>
          <span>MY LIBRARY</span>
          <h1>Release calendar</h1>
          <p>Movies and new episodes from titles saved to this profile.</p>
        </div>
      </header>
      <div className="calendar-toolbar">
        <button aria-label="Previous month" onClick={() => changeMonth(-1)}>‹</button>
        <h2>{monthTitle.format(visibleMonth)}</h2>
        <button className="calendar-today" onClick={goToday}>Today</button>
        <button aria-label="Next month" onClick={() => changeMonth(1)}>›</button>
      </div>
      <div className="calendar-layout">
        <section key={prefix} className={`calendar-board calendar-month-${monthDirection}`} aria-label={monthTitle.format(visibleMonth)}>
          {loading && !monthReleases.length ? (
            <div className="calendar-month-loading" role="status" aria-live="polite">
              <i className="loading-spinner" />
              <span>Loading {monthTitle.format(visibleMonth)}</span>
            </div>
          ) : <>
            <div className="calendar-weekdays">
              {WEEKDAYS.map((day) => <span key={day}>{day}</span>)}
            </div>
            <div className="calendar-grid">
            {cells.map((day, index) => {
              if (day == null) return <span className="calendar-cell empty" key={`empty-${index}`} />;
              const date = `${prefix}-${String(day).padStart(2, "0")}`;
              const dayItems = releasesByDate.get(date) ?? [];
              return (
                <button
                  key={date}
                  className={[
                    "calendar-cell",
                    date === today ? "today" : "",
                    date === selectedDate ? "selected" : "",
                    dayItems.length ? "has-releases" : "",
                  ].filter(Boolean).join(" ")}
                  onClick={() => setSelectedDate(date)}
                >
                  <span className="calendar-day-number">{day}</span>
                  <span className={`calendar-cell-thumbnails${dayItems.length > 1 ? " has-multiple" : ""}`}>
                    {dayItems.slice(0, 2).map((item) => {
                      const artwork = item.meta.poster ?? item.meta.background;
                      return artwork ? (
                        <img key={item.key} src={artwork} alt="" loading="lazy" />
                      ) : null;
                    })}
                    {dayItems.length > 2 && <small>+{dayItems.length - 2}</small>}
                  </span>
                  {dayItems.length > 0 && <i className="calendar-dot" />}
                </button>
              );
            })}
            </div>
          </>}
        </section>
        <aside className="calendar-agenda">
          <header>
            <span>{selectedDate === today ? "TODAY" : "RELEASES"}</span>
            <h2>{dayTitle.format(new Date(`${selectedDate}T12:00:00`))}</h2>
            <small>{selected.length} {selected.length === 1 ? "release" : "releases"}</small>
          </header>
          <div className="calendar-agenda-list">
            {selected.map((item) => (
              <button
                key={item.key}
                onClick={() => onSelect({
                  ...item.meta,
                  selectedVideoId: item.video?.id,
                })}
              >
                <span
                  className="calendar-release-art"
                  style={{ backgroundImage: `url("${item.video?.thumbnail ?? item.meta.poster ?? item.meta.background ?? ""}")` }}
                />
                <span className="calendar-release-copy">
                  <small>{item.kind === "movie" ? "MOVIE" : "NEW EPISODE"}</small>
                  <strong>{item.meta.name}</strong>
                  <span>{releaseLabel(item)}</span>
                </span>
                <span className="calendar-open">›</span>
              </button>
            ))}
            {!selected.length && (
              <div className="calendar-empty-day">
                <strong>No releases this day</strong>
                <span>{loading ? "Still checking your library…" : "Pick a highlighted day or change the month."}</span>
              </div>
            )}
          </div>
        </aside>
      </div>
      {ready && !items.length && (
        <div className="calendar-empty-library">
          <strong>Your calendar is empty</strong>
          <span>Add movies or series to your library to track their releases.</span>
        </div>
      )}
    </div>
  );
}

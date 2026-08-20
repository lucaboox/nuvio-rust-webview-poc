import assert from "node:assert/strict";
import test from "node:test";
import {
  buildReleaseCalendar,
  localReleaseDate,
  monthCells,
} from "./releaseCalendar.ts";

function meta(overrides = {}) {
  return {
    id: "tt-show",
    contentType: "series",
    name: "Example Show",
    genres: [],
    cast: [],
    director: [],
    writer: [],
    trailers: [],
    externalRatings: [],
    videos: [],
    sourceManifestUrl: "https://example.com/manifest.json",
    addonName: "Metadata",
    hasScheduledVideos: false,
    ...overrides,
  };
}

test("calendar groups dated library movies and episodes without duplicates", () => {
  const episode = {
    id: "tt-show:1:2",
    title: "Second",
    season: 1,
    episode: 2,
    released: "2026-08-19",
    available: true,
  };
  const entries = buildReleaseCalendar([
    meta({ videos: [episode, episode] }),
    meta({
      id: "tt-movie",
      contentType: "movie",
      name: "Movie",
      released: "2026-08-18",
    }),
  ]);
  assert.deepEqual(
    entries.map((item) => [item.date, item.kind, item.video?.id]),
    [
      ["2026-08-18", "movie", undefined],
      ["2026-08-19", "episode", "tt-show:1:2"],
    ],
  );
});

test("plain release dates are validated and month cells align to weekdays", () => {
  assert.equal(localReleaseDate("2026-02-29"), null);
  assert.equal(localReleaseDate("2028-02-29"), "2028-02-29");
  const cells = monthCells(2026, 7);
  assert.equal(cells.slice(0, 6).every((day) => day === null), true);
  assert.equal(cells[6], 1);
  assert.equal(cells.filter(Boolean).length, 31);
  const mondayFirst = monthCells(2026, 7, true);
  assert.equal(mondayFirst.slice(0, 5).every((day) => day === null), true);
  assert.equal(mondayFirst[5], 1);
});

import assert from "node:assert/strict";
import test from "node:test";

import { currentSeriesVideo } from "./seriesProgress.ts";

const videos = [
  { id: "s1e1", season: 1, episode: 1 },
  { id: "s1e2", season: 1, episode: 2 },
  { id: "s2e1", season: 2, episode: 1 },
  { id: "s2e2", season: 2, episode: 2 },
];

test("details default to the season containing the active resume", () => {
  const current = currentSeriesVideo(videos, "show", {
    entries: [{
      contentId: "show", contentType: "series", videoId: "s2e1",
      season: 2, episode: 1, positionMs: 10_000, durationMs: 40_000,
      lastWatched: 100,
    }],
    watchedItems: [],
  });
  assert.equal(current?.id, "s2e1");
});

test("details cross into the next season after a watched finale", () => {
  const current = currentSeriesVideo(videos, "show", {
    entries: [],
    watchedItems: videos.slice(0, 2).map((video, index) => ({
      contentId: "show", contentType: "series", title: "Show",
      season: video.season, episode: video.episode, watchedAt: index + 1,
    })),
  });
  assert.equal(current?.id, "s2e1");
});

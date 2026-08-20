import assert from "node:assert/strict";
import test from "node:test";

import {
  artworkForContinueWatching,
  buildContinueWatching,
  CONTINUE_WATCHING_DEFAULTS,
  splitContinueWatching,
} from "./continueWatching.ts";

const meta = (videos) => ({
  id: "show",
  contentType: "series",
  name: "Show",
  poster: "poster.jpg",
  background: "background.jpg",
  genres: [],
  cast: [],
  director: [],
  writer: [],
  trailers: [],
  externalRatings: [],
  hasScheduledVideos: false,
  videos,
  sourceManifestUrl: "https://example.invalid/manifest.json",
  addonName: "Test",
});

const watched = (season, episode, watchedAt) => ({
  contentId: "show",
  contentType: "series",
  title: "Show",
  season,
  episode,
  watchedAt,
});

test("Continue Watching visibility, unaired and dismiss settings affect construction", () => {
  const item = meta([
    { id: "e1", title: "One", season: 1, episode: 1, available: true },
    {
      id: "e2",
      title: "Two",
      season: 1,
      episode: 2,
      available: true,
      released: "2099-01-01T00:00:00.000Z",
    },
  ]);
  const progress = { entries: [], watchedItems: [watched(1, 1, 10)] };
  assert.equal(buildContinueWatching(progress, [item]).length, 1);
  assert.equal(
    buildContinueWatching(progress, [item], {
      ...CONTINUE_WATCHING_DEFAULTS,
      continueWatchingVisible: false,
    }).length,
    0,
  );
  assert.equal(
    buildContinueWatching(progress, [item], {
      ...CONTINUE_WATCHING_DEFAULTS,
      continueWatchingShowUnairedNextUp: false,
    }).length,
    0,
  );
  assert.equal(
    buildContinueWatching(progress, [item], {
      ...CONTINUE_WATCHING_DEFAULTS,
      dismissedNextUp: ["show|1|1"],
    }).length,
    0,
  );
});

test("furthest episode and latest activity select different Next Up seeds", () => {
  const item = meta([
    { id: "s1e1", title: "S1 One", season: 1, episode: 1, available: true },
    { id: "s1e2", title: "S1 Two", season: 1, episode: 2, available: true },
    { id: "s2e1", title: "S2 One", season: 2, episode: 1, available: true },
    { id: "s2e2", title: "S2 Two", season: 2, episode: 2, available: true },
  ]);
  const progress = {
    entries: [],
    watchedItems: [watched(2, 1, 10), watched(1, 1, 20)],
  };
  assert.equal(buildContinueWatching(progress, [item])[0].video.id, "s2e2");
  assert.equal(
    buildContinueWatching(progress, [item], {
      ...CONTINUE_WATCHING_DEFAULTS,
      continueWatchingUpNextFromFurthestEpisode: false,
    })[0].video.id,
    "s1e2",
  );
});

test("split upcoming and artwork styles match their visible settings", () => {
  const item = meta([
    { id: "e1", title: "One", season: 1, episode: 1, available: true },
    {
      id: "e2",
      title: "Two",
      season: 1,
      episode: 2,
      available: true,
      thumbnail: "episode.jpg",
      released: "2099-01-01T00:00:00.000Z",
    },
  ]);
  const cards = buildContinueWatching(
    { entries: [], watchedItems: [watched(1, 1, 10)] },
    [item],
  );
  const split = splitContinueWatching(cards, "SPLIT_UPCOMING");
  assert.equal(split.current.length, 0);
  assert.equal(split.upcoming.length, 1);
  assert.equal(artworkForContinueWatching(cards[0]), "episode.jpg");
  assert.equal(
    artworkForContinueWatching(cards[0], {
      ...CONTINUE_WATCHING_DEFAULTS,
      continueWatchingStyle: "Poster",
    }),
    "poster.jpg",
  );
  assert.equal(
    artworkForContinueWatching(cards[0], {
      ...CONTINUE_WATCHING_DEFAULTS,
      continueWatchingUseEpisodeThumbnails: false,
    }),
    "background.jpg",
  );
});

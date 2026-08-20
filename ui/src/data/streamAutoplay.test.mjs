import assert from "node:assert/strict";
import test from "node:test";
import {
  autoplayCandidates,
  selectInitialAutoplay,
  selectPreferredBingeGroup,
} from "./streamAutoplay.ts";

const streams = [
  {
    name: "Episode source",
    title: "",
    description: "",
    url: "https://video.example/episode.mkv",
    sources: [],
    addonName: "Example addon",
    behaviorHints: { bingeGroup: "example-release" },
  },
];

test("manual episode clicks ignore a remembered next-episode binge group", () => {
  const settings = {
    autoplayMode: "MANUAL",
    autoplayPreferBingeGroup: true,
    autoplayReuseBingeGroup: true,
  };
  const candidates = autoplayCandidates(streams, settings, false);

  assert.equal(
    selectPreferredBingeGroup(candidates, "example-release"),
    streams[0],
    "the fixture must contain the remembered group that caused the regression",
  );
  assert.equal(selectInitialAutoplay(candidates, settings), null);
});

test("explicit first-stream autoplay still applies to an initial episode click", () => {
  assert.equal(
    selectInitialAutoplay(streams, { autoplayMode: "FIRST_STREAM" }),
    streams[0],
  );
});

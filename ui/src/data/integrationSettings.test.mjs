import assert from "node:assert/strict";
import test from "node:test";

import {
  INTEGRATION_CREDENTIAL_KEY,
  normalizeIntegrationCredential,
} from "./integrationSettings.ts";

test("provider IDs map to the credential keys returned by the native bridge", () => {
  assert.deepEqual(INTEGRATION_CREDENTIAL_KEY, {
    tmdb: "tmdbApiKey",
    mdblist: "mdbListApiKey",
    animeskip: "animeSkipClientId",
    introdb: "introDbApiKey",
    "debrid:torbox": "torboxApiKey",
    "debrid:premiumize": "premiumizeApiKey",
    "debrid:realdebrid": "realDebridApiKey",
  });
});

test("credential edits use the same trimmed value as official Nuvio", () => {
  assert.equal(normalizeIntegrationCredential("  key-value  "), "key-value");
  assert.equal(normalizeIntegrationCredential("   "), "");
});

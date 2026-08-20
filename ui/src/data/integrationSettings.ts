export type IntegrationProvider =
  | "tmdb"
  | "mdblist"
  | "animeskip"
  | "introdb"
  | "debrid:torbox"
  | "debrid:premiumize"
  | "debrid:realdebrid";
export type IntegrationCredentialKey =
  | "tmdbApiKey"
  | "mdbListApiKey"
  | "animeSkipClientId"
  | "introDbApiKey"
  | "torboxApiKey"
  | "premiumizeApiKey"
  | "realDebridApiKey";

export const INTEGRATION_CREDENTIAL_KEY: Record<
  IntegrationProvider,
  IntegrationCredentialKey
> = {
  tmdb: "tmdbApiKey",
  mdblist: "mdbListApiKey",
  animeskip: "animeSkipClientId",
  introdb: "introDbApiKey",
  "debrid:torbox": "torboxApiKey",
  "debrid:premiumize": "premiumizeApiKey",
  "debrid:realdebrid": "realDebridApiKey",
};

export function normalizeIntegrationCredential(value: unknown): string {
  return String(value ?? "").trim();
}

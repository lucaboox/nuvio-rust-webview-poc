export type BridgeRequest = {
  id: string;
  method: string;
  params: unknown;
};

export type BridgeResponse<T = unknown> = {
  id: string;
  ok: boolean;
  result?: T;
  error?: {
    code: string;
    message: string;
  };
};

export type BridgeEvent<T = unknown> = {
  event: string;
  payload: T;
};

export type BootstrapResult = {
  appName: string;
  architecture: string;
  platform: string;
  protocolVersion: number;
  auth: AuthSnapshot;
  profiles: NuvioProfile[];
  activeProfileIndex: number;
  addons: AddonRow[];
  settings?: SettingsSnapshot | null;
  player: {
    backend: string;
    directMpvReady: boolean;
    integration: string;
  };
};

export type AuthSnapshot = {
  status: "authenticated" | "unauthenticated";
  backendConfigured: boolean;
  officialBackendConfigured: boolean;
  selfHosted: boolean;
  backendUrl?: string;
  customKeySaved: boolean;
  userId?: string;
  email?: string;
  isAnonymous: boolean;
};

export type NuvioProfile = {
  id: string;
  userId: string;
  profileIndex: number;
  name: string;
  avatarColorHex: string;
  avatarId?: string;
  avatarUrl?: string;
  usesPrimaryAddons: boolean;
  usesPrimaryPlugins: boolean;
  pinEnabled: boolean;
};

export type AvatarCatalogItem = {
  id: string;
  displayName: string;
  storagePath: string;
  category: string;
  sortOrder: number;
  isActive: boolean;
  bgColor?: string;
  imageUrl: string;
};

export type AddonRow = {
  url: string;
  name?: string;
  enabled: boolean;
  sortOrder: number;
};

export type AccountPayload = {
  auth: AuthSnapshot;
  profiles: NuvioProfile[];
  activeProfileIndex: number;
  addons: AddonRow[];
  settings?: SettingsSnapshot | null;
  warning?: string;
};

export type Video = {
  id: string;
  title: string;
  season?: number;
  episode?: number;
  released?: string;
  thumbnail?: string;
  seasonPoster?: string;
  overview?: string;
  runtime?: number;
  available: boolean;
};

export type MetaPerson = {
  name: string;
  role?: string;
  photo?: string;
  tmdbId?: number;
};
export type PersonCredit = {
  id: string;
  contentType: string;
  name: string;
  poster: string;
  background?: string;
  description?: string;
  releaseInfo?: string;
  rawReleaseDate?: string;
  popularity?: number;
};
export type PersonDetail = {
  tmdbId: number;
  name: string;
  biography?: string;
  birthday?: string;
  deathday?: string;
  placeOfBirth?: string;
  profilePhoto?: string;
  knownFor?: string;
  movieCredits: PersonCredit[];
  tvCredits: PersonCredit[];
};
export type MetaTrailer = {
  id: string;
  key: string;
  name: string;
  site: string;
  size?: number;
  trailerType: string;
  official?: boolean;
  publishedAt?: string;
  seasonNumber?: number;
  displayName?: string;
};
export type ExternalRating = { source: string; value: number };

export type ContentMeta = {
  id: string;
  contentType: string;
  name: string;
  poster?: string;
  background?: string;
  banner?: string;
  logo?: string;
  posterShape?: string;
  description?: string;
  releaseInfo?: string;
  released?: string;
  imdbRating?: string;
  genres: string[];
  runtime?: string;
  cast: MetaPerson[];
  director: string[];
  writer: string[];
  status?: string;
  ageRating?: string;
  lastAirDate?: string;
  country?: string;
  awards?: string;
  language?: string;
  website?: string;
  trailers: MetaTrailer[];
  externalRatings: ExternalRating[];
  defaultVideoId?: string;
  selectedVideoId?: string;
  hasScheduledVideos: boolean;
  videos: Video[];
  sourceManifestUrl: string;
  addonName: string;
};

export type LibraryItem = ContentMeta & { addedAt: number };
export type ResumePoint = {
  contentId: string;
  contentType: string;
  videoId: string;
  season?: number;
  episode?: number;
  positionMs: number;
  durationMs: number;
  lastWatched: number;
};
export type WatchedItem = {
  contentId: string;
  contentType: string;
  title: string;
  season?: number;
  episode?: number;
  watchedAt: number;
};
export type ProgressSnapshot = {
  entries: ResumePoint[];
  watchedItems: WatchedItem[];
};

export type CollectionCatalogSource = {
  addonId: string;
  type: string;
  catalogId: string;
  genre?: string;
};
export type CollectionSource = {
  provider: string;
  addonId?: string;
  type?: string;
  catalogId?: string;
  genre?: string;
  tmdbSourceType?: string;
  title?: string;
  tmdbId?: number;
  traktListId?: number;
  mediaType?: string;
  sortBy?: string;
  sortHow?: string;
};
export type CollectionFolder = {
  id: string;
  title: string;
  coverImageUrl?: string;
  focusGifUrl?: string;
  focusGifEnabled: boolean;
  coverEmoji?: string;
  tileShape: string;
  hideTitle: boolean;
  sources: CollectionSource[];
  catalogSources: CollectionCatalogSource[];
  heroBackdropUrl?: string;
  heroVideoUrl?: string;
  titleLogoUrl?: string;
};
export type NuvioCollection = {
  id: string;
  title: string;
  backdropImageUrl?: string;
  pinToTop: boolean;
  viewMode: string;
  showAllTab: boolean;
  folders: CollectionFolder[];
};
export type AvailableCollectionCatalog = {
  addonId: string;
  addonName: string;
  contentType: string;
  catalogId: string;
  catalogName: string;
  genreOptions: string[];
  genreRequired: boolean;
};

export type DiscoverCatalog = {
  key: string;
  addonName: string;
  manifestUrl: string;
  contentType: string;
  catalogId: string;
  catalogName: string;
  genreOptions: string[];
  genreRequired: boolean;
  supportsPagination: boolean;
};

export type CatalogSection = {
  key: string;
  /** Nuvio home-layout key (`{manifestId}:{type}:{catalogId}`). */
  prefKey: string;
  title: string;
  subtitle: string;
  manifestUrl: string;
  contentType: string;
  catalogId: string;
  genre?: string;
  items: ContentMeta[];
};

/** One row of the home page, in the user's configured order. */
export type HomeLayoutRow = {
  key: string;
  isCollection: boolean;
  collectionId?: string;
};

export type HomePayload = {
  sections: CatalogSection[];
  rows: HomeLayoutRow[];
  hero?: ContentMeta;
  heroItems?: ContentMeta[];
  errors: string[];
};

export type HomeLayoutItem = {
  key: string;
  defaultTitle: string;
  displayTitle: string;
  customTitle: string;
  subtitle: string;
  enabled: boolean;
  heroSourceEnabled: boolean;
  order: number;
  isCollection: boolean;
  collectionId?: string;
  pinnedToTop: boolean;
};

export type HomeLayoutState = {
  heroEnabled: boolean;
  showCatalogType: boolean;
  hideUnreleasedContent: boolean;
  heroSourceLimit: number;
  /** Rows kept for addons/collections this device cannot see. */
  preservedCount: number;
  items: HomeLayoutItem[];
};

export type HomeLayoutAction =
  | { action: "setEnabled"; key: string; enabled: boolean }
  | { action: "setHeroSourceEnabled"; key: string; enabled: boolean }
  | { action: "setCustomTitle"; key: string; title: string }
  | { action: "setHeroEnabled"; enabled: boolean }
  | { action: "setShowCatalogType"; enabled: boolean }
  | { action: "setHideUnreleasedContent"; enabled: boolean }
  | { action: "move"; from: number; to: number }
  | { action: "reset" };

export type StreamSource = {
  name: string;
  title: string;
  description: string;
  url?: string;
  infoHash?: string;
  fileIdx?: number;
  externalUrl?: string;
  sources: string[];
  addonName: string;
  addonId: string;
  streamType?: string;
  behaviorHints?: {
    bingeGroup?: string;
    videoHash?: string;
    videoSize?: number;
    filename?: string;
    notWebReady: boolean;
    proxyHeaders?: {
      request?: Record<string, string>;
      response?: Record<string, string>;
    };
  };
  addonLogo?: string;
};

export type SkipSegment = {
  startMs: number;
  endMs: number;
  type: string;
  provider: string;
};

export type DownloadItem = {
  id: string;
  contentId: string;
  contentType: string;
  videoId: string;
  title: string;
  showName?: string;
  season?: number;
  episode?: number;
  sourceName: string;
  status: "queued" | "downloading" | "completed" | "failed" | "cancelled";
  bytesDownloaded: number;
  totalBytes?: number;
  filePath?: string;
  playUrl?: string;
  artworkCached: boolean;
  error?: string;
  createdAt: number;
  skipSegments: SkipSegment[];
};

export type DownloadsSnapshot = {
  root: string;
  items: DownloadItem[];
};

export type AddonDescriptor = {
  url: string;
  name: string;
  /** Manifest version; empty when the addon could not be reached. */
  version: string;
  enabled: boolean;
  sortOrder: number;
  configurable: boolean;
  configurationRequired: boolean;
  configureUrl?: string;
  catalogCount: number;
  resourceNames: string[];
  logo?: string;
  error?: string;
};

export type SettingsSnapshot = {
  amoledEnabled: boolean;
  showLoadingOverlay: boolean;
  showParentalGuide: boolean;
  resizeMode: "Fit" | "Fill" | "Zoom" | "Stretch";
  preferredAudioLanguage: string;
  preferredSubtitleLanguage: string;
  subtitleBold: boolean;
  subtitleFontSize: number;
  subtitleOutline: boolean;
  reuseLastStream: boolean;
  reuseLastStreamHours: number;
  autoplayMode: "MANUAL" | "FIRST_STREAM" | "REGEX_MATCH";
  autoplayNextEpisode: boolean;
  skipIntro: boolean;
  rtxSuperResolution: boolean;
  showFileSizeBadges: boolean;
  badgePlacement: "TOP" | "BOTTOM";
  episodeReleaseAlerts: boolean;
  /** Poster card style — synced via features.poster_card_style_settings_payload. */
  posterWidth: number;
  posterCornerRadius: number;
  posterHideLabels: boolean;
  posterLandscapeCatalogs: boolean;
  nextEpisodeThresholdMode: "PERCENTAGE" | "MINUTES_BEFORE_END";
  nextEpisodeThresholdPercent: number;
  nextEpisodeThresholdMinutes: number;
  // Subtitle appearance
  subtitleTextColor: string;
  subtitleBackgroundColor: string;
  subtitleOutlineColor: string;
  subtitleOutlineWidth: number;
  subtitleBottomOffset: number;
  subtitleForcedOnly: boolean;
  subtitlePreferredLanguagesOnly: boolean;
  secondaryAudioLanguage: string;
  secondarySubtitleLanguage: string;
  addonSubtitleStartupMode: "ALL_SUBTITLES" | "PREFERRED_ONLY" | "NONE";
  useLibass: boolean;
  // Autoplay
  autoplaySource: string;
  autoplaySelectedAddons: string[];
  autoplaySelectedPlugins: string[];
  autoplayRegex: string;
  autoplayTimeoutSeconds: number;
  autoplayPreferBingeGroup: boolean;
  autoplayReuseBingeGroup: boolean;
  autoplayNextEpisodeFallback: boolean;
  // Skipping
  animeSkipEnabled: boolean;
  animeSkipClientId: string;
  introDbApiKey: string;
  introSubmitEnabled: boolean;
  // Gestures (synced by Nuvio)
  holdToSpeed: boolean;
  holdToSpeedValue: number;
  // External player
  externalPlayerEnabled: boolean;
  externalPlayerId: string;
  externalPlayerForwardSubtitles: boolean;
  externalPlayerSendSkipSegments: boolean;
};

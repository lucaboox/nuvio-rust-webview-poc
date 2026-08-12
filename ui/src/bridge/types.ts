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
  player: {
    backend: string;
    directMpvReady: boolean;
    integration: string;
  };
};

export type AuthSnapshot = {
  status: "authenticated" | "unauthenticated";
  backendConfigured: boolean;
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

export type MetaPerson = { name: string; role?: string; photo?: string; tmdbId?: number };
export type MetaTrailer = { id: string; key: string; name: string; site: string; trailerType: string; official?: boolean; publishedAt?: string; seasonNumber?: number };
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
export type ResumePoint = { contentId: string; contentType: string; videoId: string; season?: number; episode?: number; positionMs: number; durationMs: number; lastWatched: number };
export type WatchedItem = { contentId: string; contentType: string; title: string; season?: number; episode?: number; watchedAt: number };
export type ProgressSnapshot = { entries: ResumePoint[]; watchedItems: WatchedItem[] };

export type CollectionCatalogSource = { addonId: string; type: string; catalogId: string; genre?: string };
export type CollectionSource = { provider: string; addonId?: string; type?: string; catalogId?: string; genre?: string; tmdbSourceType?: string; title?: string; tmdbId?: number; traktListId?: number; mediaType?: string };
export type CollectionFolder = { id: string; title: string; coverImageUrl?: string; focusGifUrl?: string; focusGifEnabled: boolean; coverEmoji?: string; tileShape: string; hideTitle: boolean; sources: CollectionSource[]; catalogSources: CollectionCatalogSource[]; heroBackdropUrl?: string; heroVideoUrl?: string; titleLogoUrl?: string };
export type NuvioCollection = { id: string; title: string; backdropImageUrl?: string; pinToTop: boolean; viewMode: string; showAllTab: boolean; folders: CollectionFolder[] };

export type CatalogSection = {
  key: string;
  title: string;
  subtitle: string;
  manifestUrl: string;
  contentType: string;
  catalogId: string;
  genre?: string;
  items: ContentMeta[];
};

export type HomePayload = {
  sections: CatalogSection[];
  hero?: ContentMeta;
  heroItems?: ContentMeta[];
  errors: string[];
};

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
  behaviorHints?: { bingeGroup?: string; videoHash?: string; videoSize?: number; filename?: string; notWebReady: boolean; proxyHeaders?: { request?: Record<string, string>; response?: Record<string, string> } };
  addonLogo?: string;
};

export type AddonDescriptor = {
  url: string;
  name: string;
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
  autoplayMode: "MANUAL" | "FIRST_STREAM" | "REGEX_MATCH";
  autoplayNextEpisode: boolean;
  skipIntro: boolean;
  rtxSuperResolution: boolean;
  showFileSizeBadges: boolean;
  badgePlacement: "TOP" | "BOTTOM";
  episodeReleaseAlerts: boolean;
};

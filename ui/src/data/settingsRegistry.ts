import type { SettingsSnapshot } from "../bridge/types";
import type { ClientSettings } from "./clientSettings";
import { POSTER_RADII, POSTER_WIDTHS } from "./posterSize";

/**
 * Every setting declared once, so the page layout and the search index cannot
 * drift apart. Nuvio spreads these across 22 screens with no way to find one;
 * grouping by what you are changing — and making them searchable — is the whole
 * point of this file.
 */
export type SettingScope = "account" | "device" | "local";

export type SettingControl =
  | { kind: "switch" }
  | { kind: "preset"; options: readonly (readonly [string, string | number])[] }
  | { kind: "number"; min: number; max: number; step?: number; suffix?: string }
  | { kind: "text"; placeholder?: string; secret?: boolean }
  | { kind: "color" };

export type SettingDef = {
  /** Key on SettingsSnapshot, or on ClientSettings when scope is "local". */
  id: keyof SettingsSnapshot | keyof ClientSettings;
  label: string;
  detail?: string;
  /** Extra words that should match in search but do not belong in the label. */
  keywords?: string;
  control: SettingControl;
  /** Hide unless another setting is on — keeps dependent options out of the way. */
  requires?: keyof SettingsSnapshot | keyof ClientSettings;
};

export type SettingGroup = {
  title: string;
  subtitle: string;
  settings: SettingDef[];
};

export type SettingSection = {
  id: string;
  label: string;
  icon: string;
  scope: SettingScope;
  subtitle: string;
  groups: SettingGroup[];
  /** Sections whose body is a bespoke component rather than a control list. */
  custom?: "homeLayout" | "collections" | "downloads" | "updates";
};

// Nuvio's AvailableLanguageOptions, verbatim — same codes, same English
// labels. The previous list was eight invented entries, so most of what the
// other clients offer simply could not be chosen here.
const LANGUAGES = [
  ["Afrikaans", "af"],
  ["Albanian", "sq"],
  ["Amharic", "am"],
  ["Arabic", "ar"],
  ["Armenian", "hy"],
  ["Azerbaijani", "az"],
  ["Basque", "eu"],
  ["Belarusian", "be"],
  ["Bengali", "bn"],
  ["Bosnian", "bs"],
  ["Bulgarian", "bg"],
  ["Burmese", "my"],
  ["Catalan", "ca"],
  ["Chinese", "zh"],
  ["Chinese (Simplified)", "zh-CN"],
  ["Chinese (Traditional)", "zh-TW"],
  ["Croatian", "hr"],
  ["Czech", "cs"],
  ["Danish", "da"],
  ["Dutch", "nl"],
  ["English", "en"],
  ["Estonian", "et"],
  ["Filipino", "tl"],
  ["Finnish", "fi"],
  ["French", "fr"],
  ["Galician", "gl"],
  ["Georgian", "ka"],
  ["German", "de"],
  ["Greek", "el"],
  ["Gujarati", "gu"],
  ["Hebrew", "he"],
  ["Hindi", "hi"],
  ["Hungarian", "hu"],
  ["Icelandic", "is"],
  ["Indonesian", "id"],
  ["Irish", "ga"],
  ["Italian", "it"],
  ["Japanese", "ja"],
  ["Kannada", "kn"],
  ["Kazakh", "kk"],
  ["Khmer", "km"],
  ["Korean", "ko"],
  ["Lao", "lo"],
  ["Latvian", "lv"],
  ["Lithuanian", "lt"],
  ["Macedonian", "mk"],
  ["Malay", "ms"],
  ["Malayalam", "ml"],
  ["Maltese", "mt"],
  ["Marathi", "mr"],
  ["Mongolian", "mn"],
  ["Nepali", "ne"],
  ["Norwegian", "no"],
  ["Punjabi", "pa"],
  ["Persian", "fa"],
  ["Polish", "pl"],
  ["Portuguese (Portugal)", "pt"],
  ["Portuguese (Brazil)", "pt-BR"],
  ["Romanian", "ro"],
  ["Russian", "ru"],
  ["Serbian", "sr"],
  ["Sinhala", "si"],
  ["Slovak", "sk"],
  ["Slovenian", "sl"],
  ["Spanish", "es"],
  ["Spanish (Latin America)", "es-419"],
  ["Swahili", "sw"],
  ["Swedish", "sv"],
  ["Tamil", "ta"],
  ["Telugu", "te"],
  ["Thai", "th"],
  ["Turkish", "tr"],
  ["Ukrainian", "uk"],
  ["Urdu", "ur"],
  ["Uzbek", "uz"],
  ["Vietnamese", "vi"],
  ["Welsh", "cy"],
  ["Zulu", "zu"],
] as const;

// The special values and their wording also come from Nuvio
// (PlayerLanguagePreferences: AudioLanguageOption / SubtitleLanguageOption).
// "Off" was ours; theirs is "None", and the stored code is "none".
const AUDIO_LANGUAGES = [
  ["Default (media file)", "default"],
  ["Device language", "device"],
  ["Original language", "original"],
  ...LANGUAGES,
] as const;

const SUBTITLE_LANGUAGES = [
  ["None", "none"],
  ["Device language", "device"],
  ["Forced", "forced"],
  ...LANGUAGES,
] as const;

// A secondary preference is genuinely unset rather than "none", so the empty
// string stays as its own entry.
const OPTIONAL_LANGUAGES = [["None", ""], ...LANGUAGES] as const;

export const SECTIONS: SettingSection[] = [
  {
    id: "home",
    label: "Home Layout",
    icon: "home",
    scope: "account",
    subtitle: "Which rows appear on Home, and in what order.",
    custom: "homeLayout",
    groups: [],
  },
  {
    id: "collections",
    label: "Collections",
    icon: "collections",
    scope: "account",
    subtitle: "Collections and folders synced from your account.",
    custom: "collections",
    groups: [],
  },
  {
    id: "appearance",
    label: "Appearance",
    icon: "settings",
    scope: "device",
    subtitle: "Theme and how posters are drawn.",
    groups: [
      {
        title: "Theme",
        subtitle: "Surface colours across the client.",
        settings: [
          {
            id: "amoledEnabled",
            label: "AMOLED black",
            detail: "Pure black surfaces for OLED displays",
            keywords: "dark oled contrast",
            control: { kind: "switch" },
          },
        ],
      },
      {
        title: "Poster cards",
        subtitle: "Card size and shape, matching Nuvio's presets.",
        settings: [
          {
            id: "posterWidth",
            label: "Width",
            detail: "Height follows at a 2:3 ratio",
            keywords: "size poster card big small",
            control: { kind: "preset", options: POSTER_WIDTHS },
          },
          {
            id: "posterCornerRadius",
            label: "Corner radius",
            detail: "Roundness of poster artwork",
            keywords: "rounded sharp corners",
            control: { kind: "preset", options: POSTER_RADII },
          },
          {
            id: "posterHideLabels",
            label: "Hide labels",
            detail: "Drop the title and year beneath each poster",
            control: { kind: "switch" },
          },
          {
            id: "posterLandscapeCatalogs",
            label: "Landscape posters",
            detail: "Wide artwork in the full catalog view",
            control: { kind: "switch" },
          },
        ],
      },
    ],
  },
  {
    id: "playback",
    label: "Playback",
    icon: "play",
    scope: "device",
    subtitle: "Picture, autoplay and skipping.",
    groups: [
      {
        title: "Picture",
        subtitle: "How video is framed and what the player shows while it works.",
        settings: [
          {
            id: "resizeMode",
            label: "Video sizing",
            detail: "How video fills the window",
            keywords: "aspect ratio fit fill zoom stretch crop",
            control: {
              kind: "preset",
              options: [
                ["Fit", "Fit"],
                ["Fill", "Fill"],
                ["Zoom", "Zoom"],
                ["Stretch", "Stretch"],
              ],
            },
          },
          {
            id: "rtxSuperResolution",
            label: "RTX video enhancement",
            detail: "NVIDIA RTX super resolution for libmpv",
            keywords: "nvidia upscale ai",
            control: { kind: "switch" },
          },
          {
            id: "showLoadingOverlay",
            label: "Loading overlay",
            detail: "Show status while opening a stream",
            control: { kind: "switch" },
          },
          {
            id: "showParentalGuide",
            label: "Parental guide",
            detail: "Show content advisories where available",
            keywords: "age rating warning",
            control: { kind: "switch" },
          },
        ],
      },
      {
        title: "Autoplay",
        subtitle: "How a source is chosen when you press play.",
        settings: [
          {
            id: "autoplayMode",
            label: "Source selection",
            detail: "What happens when you press play",
            keywords: "automatic first stream regex",
            control: {
              kind: "preset",
              options: [
                ["Manual", "MANUAL"],
                ["First stream", "FIRST_STREAM"],
                ["Pattern match", "REGEX_MATCH"],
              ],
            },
          },
          {
            id: "autoplayRegex",
            label: "Match pattern",
            detail: "Regular expression tried against each source name",
            keywords: "regex filter quality 1080p",
            control: { kind: "text", placeholder: "e.g. 1080p.*(WEB|BluRay)" },
          },
          {
            id: "autoplaySource",
            label: "Look in",
            detail: "Which providers autoplay may pick from",
            control: {
              kind: "preset",
              options: [
                ["All sources", "ALL_SOURCES"],
                ["Installed addons only", "INSTALLED_ADDONS_ONLY"],
                ["Plugins only", "ENABLED_PLUGINS_ONLY"],
              ],
            },
          },
          {
            id: "autoplayTimeoutSeconds",
            label: "Wait before choosing",
            detail: "Seconds to gather sources before auto-selecting",
            keywords: "delay timeout",
            control: { kind: "number", min: 0, max: 30, suffix: "s" },
          },
          {
            id: "reuseLastStream",
            label: "Reuse last stream",
            detail: "Skip the source list and replay the link you last used",
            keywords: "cache link remember",
            control: { kind: "switch" },
          },
          {
            id: "reuseLastStreamHours",
            label: "Keep links for",
            detail: "Links with an expiry in the URL are never reused",
            requires: "reuseLastStream",
            control: {
              kind: "preset",
              options: [
                ["1 hour", 1],
                ["6 hours", 6],
                ["24 hours", 24],
                ["3 days", 72],
                ["7 days", 168],
              ],
            },
          },
        ],
      },
      {
        title: "Next episode",
        subtitle: "When the up-next card appears and what plays after it.",
        settings: [
          {
            id: "autoplayNextEpisode",
            label: "Play next automatically",
            detail: "Start the next episode without asking",
            keywords: "binge continue",
            control: { kind: "switch" },
          },
          {
            id: "nextEpisodeThresholdMode",
            label: "Show card by",
            detail: "Whether the card is timed by percentage or minutes left",
            control: {
              kind: "preset",
              options: [
                ["Percentage watched", "PERCENTAGE"],
                ["Minutes remaining", "MINUTES_BEFORE_END"],
              ],
            },
          },
          {
            id: "nextEpisodeThresholdPercent",
            label: "Percent watched",
            detail: "Nuvio allows 97–100%",
            control: { kind: "number", min: 97, max: 100, step: 0.5, suffix: "%" },
          },
          {
            id: "nextEpisodeThresholdMinutes",
            label: "Minutes before end",
            detail: "Nuvio allows up to 3.5 minutes",
            control: { kind: "number", min: 0, max: 3.5, step: 0.5, suffix: "m" },
          },
          {
            id: "autoplayPreferBingeGroup",
            label: "Prefer same release group",
            detail: "Keep quality consistent across an episode run",
            keywords: "binge group consistent",
            control: { kind: "switch" },
          },
          {
            id: "autoplayReuseBingeGroup",
            label: "Remember release group",
            detail: "Use the last binge group when you return to a series",
            control: { kind: "switch" },
          },
          {
            id: "autoplayNextEpisodeFallback",
            label: "Fall back to any source",
            detail: "If the preferred group has nothing, use anything playable",
            control: { kind: "switch" },
          },
        ],
      },
      {
        title: "Skipping",
        subtitle: "Intro and outro detection.",
        settings: [
          {
            id: "skipIntro",
            label: "Skip intro prompt",
            detail: "Offer to skip detected intros",
            keywords: "opening outro credits",
            control: { kind: "switch" },
          },
          {
            id: "animeSkipEnabled",
            label: "AniSkip",
            detail: "Use AniSkip timings for anime",
            keywords: "anime opening ending",
            control: { kind: "switch" },
          },
          {
            id: "animeSkipClientId",
            label: "AniSkip client ID",
            requires: "animeSkipEnabled",
            control: { kind: "text", secret: true, placeholder: "Client ID" },
          },
          {
            id: "introDbApiKey",
            label: "IntroDB API key",
            detail: "Community intro timings",
            control: { kind: "text", secret: true, placeholder: "API key" },
          },
          {
            id: "introSubmitEnabled",
            label: "Contribute timings",
            detail: "Submit intro timings you skip back to IntroDB",
            control: { kind: "switch" },
          },
        ],
      },
    ],
  },
  {
    id: "audio",
    label: "Audio & Subtitles",
    icon: "subtitles",
    scope: "device",
    subtitle: "Track selection and subtitle styling.",
    groups: [
      {
        title: "Languages",
        subtitle: "Preferred tracks to select when a stream opens.",
        settings: [
          {
            id: "preferredAudioLanguage",
            label: "Audio",
            keywords: "dub language track",
            control: { kind: "preset", options: AUDIO_LANGUAGES },
          },
          {
            id: "secondaryAudioLanguage",
            label: "Audio fallback",
            detail: "Used when the first choice is unavailable",
            control: { kind: "preset", options: OPTIONAL_LANGUAGES },
          },
          {
            id: "preferredSubtitleLanguage",
            label: "Subtitles",
            keywords: "captions cc language",
            control: { kind: "preset", options: SUBTITLE_LANGUAGES },
          },
          {
            id: "secondarySubtitleLanguage",
            label: "Subtitle fallback",
            detail: "Used when the first choice is unavailable",
            control: { kind: "preset", options: OPTIONAL_LANGUAGES },
          },
          {
            id: "subtitleForcedOnly",
            label: "Forced subtitles only",
            detail: "Show only subtitles marked forced",
            keywords: "signs songs foreign",
            control: { kind: "switch" },
          },
          {
            id: "subtitlePreferredLanguagesOnly",
            label: "Hide other languages",
            detail: "List only subtitles in your preferred languages",
            control: { kind: "switch" },
          },
          {
            id: "addonSubtitleStartupMode",
            label: "Addon subtitles at startup",
            detail: "Which addon subtitles to fetch when playback begins",
            control: {
              kind: "preset",
              options: [
                ["All", "ALL_SUBTITLES"],
                ["Preferred only", "PREFERRED_ONLY"],
                ["None", "NONE"],
              ],
            },
          },
        ],
      },
      {
        title: "Subtitle appearance",
        subtitle: "How subtitles are drawn over video.",
        settings: [
          {
            id: "subtitleFontSize",
            label: "Text size",
            control: { kind: "number", min: 12, max: 40, suffix: "px" },
          },
          {
            id: "subtitleBold",
            label: "Bold",
            control: { kind: "switch" },
          },
          {
            id: "subtitleTextColor",
            label: "Text colour",
            keywords: "colour white yellow",
            control: { kind: "color" },
          },
          {
            id: "subtitleBackgroundColor",
            label: "Background colour",
            detail: "Fully transparent by default",
            control: { kind: "color" },
          },
          {
            id: "subtitleOutline",
            label: "Outline",
            detail: "Improves readability against bright video",
            keywords: "border stroke shadow",
            control: { kind: "switch" },
          },
          {
            id: "subtitleOutlineColor",
            label: "Outline colour",
            requires: "subtitleOutline",
            control: { kind: "color" },
          },
          {
            id: "subtitleOutlineWidth",
            label: "Outline width",
            requires: "subtitleOutline",
            control: { kind: "number", min: 0, max: 8 },
          },
          {
            id: "subtitleBottomOffset",
            label: "Distance from bottom",
            keywords: "position height raise",
            control: { kind: "number", min: 0, max: 200, suffix: "px" },
          },
          {
            id: "useLibass",
            label: "Use libass rendering",
            detail: "Full ASS/SSA styling instead of plain text",
            keywords: "ass ssa styled",
            control: { kind: "switch" },
          },
        ],
      },
    ],
  },
  {
    id: "sources",
    label: "Sources",
    icon: "sources",
    scope: "device",
    subtitle: "How stream sources are presented, and external playback.",
    groups: [
      {
        title: "Source list",
        subtitle: "What each stream shows in the picker.",
        settings: [
          {
            id: "showFileSizeBadges",
            label: "File-size badges",
            detail: "Show known stream file sizes",
            keywords: "gb size quality badge",
            control: { kind: "switch" },
          },
          {
            id: "badgePlacement",
            label: "Badge placement",
            control: {
              kind: "preset",
              options: [
                ["Above title", "TOP"],
                ["Below title", "BOTTOM"],
              ],
            },
          },
        ],
      },
      {
        title: "External player",
        subtitle: "Hand playback to another application instead of the built-in player.",
        settings: [
          {
            id: "externalPlayerEnabled",
            label: "Use an external player",
            keywords: "vlc mpc potplayer",
            control: { kind: "switch" },
          },
          {
            id: "externalPlayerId",
            label: "Player",
            requires: "externalPlayerEnabled",
            control: { kind: "text", placeholder: "e.g. vlc" },
          },
          {
            id: "externalPlayerForwardSubtitles",
            label: "Forward subtitles",
            requires: "externalPlayerEnabled",
            control: { kind: "switch" },
          },
          {
            id: "externalPlayerSendSkipSegments",
            label: "Forward skip segments",
            requires: "externalPlayerEnabled",
            control: { kind: "switch" },
          },
        ],
      },
    ],
  },
  {
    id: "notifications",
    label: "Notifications",
    icon: "info",
    scope: "device",
    subtitle: "Alerts for series you follow.",
    groups: [
      {
        title: "Episodes",
        subtitle: "Releases for titles in your library.",
        settings: [
          {
            id: "episodeReleaseAlerts",
            label: "Episode release alerts",
            keywords: "notify new episode",
            control: { kind: "switch" },
          },
        ],
      },
    ],
  },
  {
    id: "client",
    label: "This Client",
    icon: "settings",
    scope: "local",
    subtitle: "Behaviour that only exists in this desktop client.",
    groups: [
      {
        title: "Player gestures",
        subtitle: "Mouse shortcuts on the video surface.",
        settings: [
          {
            id: "clickToPause",
            label: "Click to pause",
            detail: "Left click the video to pause or resume",
            keywords: "mouse click tap",
            control: { kind: "switch" },
          },
          {
            id: "seekThumbnails",
            label: "Seek bar thumbnails",
            detail: "Decode a preview frame when hovering the timeline. Opens a second connection to the source.",
            keywords: "preview scrub frame",
            control: { kind: "switch" },
          },
        ],
      },
    ],
  },
  {
    id: "downloads",
    label: "Downloads",
    icon: "downloads",
    scope: "local",
    subtitle: "Offline storage location and downloaded media files.",
    custom: "downloads",
    groups: [],
  },
  {
    id: "updates",
    label: "Updates",
    icon: "refresh",
    scope: "local",
    subtitle: "Version information and signed application updates.",
    custom: "updates",
    groups: [],
  },
];

/** Flattened index used by the search field. */
export type SearchHit = {
  section: SettingSection;
  group: SettingGroup;
  setting: SettingDef;
};

const INDEX: SearchHit[] = SECTIONS.flatMap((section) =>
  section.groups.flatMap((group) =>
    group.settings.map((setting) => ({ section, group, setting })),
  ),
);

export function searchSettings(query: string): SearchHit[] {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [];
  return INDEX.filter(({ section, group, setting }) => {
    const haystack = [
      setting.label,
      setting.detail ?? "",
      setting.keywords ?? "",
      group.title,
      section.label,
    ]
      .join(" ")
      .toLowerCase();
    return terms.every((term) => haystack.includes(term));
  }).slice(0, 24);
}

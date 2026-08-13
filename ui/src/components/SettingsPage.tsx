import { useEffect, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type {
  AvailableCollectionCatalog,
  CollectionCatalogSource,
  CollectionFolder,
  NuvioCollection,
  SettingsSnapshot,
} from "../bridge/types";
import { CollectionSettingsSection } from "./CollectionsPage";
import { HomeLayoutSection } from "./HomeLayoutPage";
import { Icon } from "./Icon";
import { POSTER_RADII, POSTER_WIDTHS } from "../data/posterSize";
import { setClientSetting, useClientSettings } from "../data/clientSettings";

const categories = [
  {
    id: "home",
    label: "Home Layout",
    icon: "home",
    scope: "account",
    subtitle: "Choose which rows appear on Home, and in what order.",
  },
  {
    id: "collections",
    label: "Collections",
    icon: "collections",
    scope: "account",
    subtitle: "Collections and folders synced from your Nuvio account.",
  },
  {
    id: "appearance",
    label: "Appearance",
    icon: "settings",
    scope: "device",
    subtitle: "How the desktop client looks.",
  },
  {
    id: "playback",
    label: "Playback",
    icon: "play",
    scope: "device",
    subtitle: "Player behavior and native playback defaults.",
  },
  {
    id: "audio",
    label: "Audio & Subtitles",
    icon: "subtitles",
    scope: "device",
    subtitle: "Language selection and subtitle rendering.",
  },
  {
    id: "client",
    label: "This Client",
    icon: "settings",
    scope: "local",
    subtitle: "Behaviour that only exists in this desktop client.",
  },
  {
    id: "streams",
    label: "Streams & Alerts",
    icon: "info",
    scope: "device",
    subtitle: "Source presentation and episode notifications.",
  },
] as const;

type CategoryId = (typeof categories)[number]["id"];

/**
 * Only two Nuvio sync surfaces are platform-scoped: the profile settings blob
 * (desktop vs mobile rows) and, deliberately shared, the home catalog layout.
 * Everything the settings blob holds therefore stops at this desktop, which is
 * worth stating plainly rather than implying it reaches the phone.
 */
const SCOPE_NOTE = {
  account: "Shared with every device on this profile — phone, TV and desktop.",
  device:
    "Stored per platform. Nuvio keeps separate desktop and mobile settings, so these do not reach your phone or TV.",
  // Not in the profile blob at all — Nuvio has no field for these.
  local:
    "Not synced anywhere. These exist only in this client and are stored on this machine.",
} as const;

const HOLD_SPEEDS = [
  ["1.5x", 1.5],
  ["2x", 2],
  ["3x", 3],
  ["4x", 4],
] as const;

/** Nuvio defaults this window to 24 hours (`streamReuseLastLinkCacheHours`). */
const REUSE_WINDOWS = [
  ["1 hour", 1],
  ["6 hours", 6],
  ["24 hours", 24],
  ["3 days", 72],
  ["7 days", 168],
] as const;

/** Shared language options for both audio and subtitle selection. */
const LANGUAGES: [string, string][] = [
  ["en", "English"],
  ["es", "Spanish"],
  ["fr", "French"],
  ["de", "German"],
  ["ja", "Japanese"],
];

export function SettingsPage({
  profileIndex,
  collections,
  availableCatalogs,
  collectionsLoading,
  collectionsError,
  onRefreshCollections,
  onReorderCollection,
  onToggleCatalog,
  onReorderCatalog,
  onOpenFolder,
  onSettingsChange,
  onHomeLayoutChanged,
}: {
  profileIndex: number;
  collections: NuvioCollection[];
  availableCatalogs: AvailableCollectionCatalog[];
  collectionsLoading: boolean;
  collectionsError: string | null;
  onRefreshCollections(): void;
  onReorderCollection(
    collectionId: string,
    folderId: string | undefined,
    direction: -1 | 1,
  ): void;
  onToggleCatalog(
    collectionId: string,
    folderId: string,
    source: CollectionCatalogSource,
  ): void;
  onReorderCatalog(
    collectionId: string,
    folderId: string,
    sourceIndex: number,
    direction: -1 | 1,
  ): void;
  onOpenFolder(collection: NuvioCollection, folder: CollectionFolder): void;
  onSettingsChange?(settings: SettingsSnapshot): void;
  onHomeLayoutChanged?(): void;
}) {
  const [settings, setSettings] = useState<SettingsSnapshot | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [category, setCategory] = useState<CategoryId>("home");
  const clientSettings = useClientSettings();

  useEffect(() => {
    setSettings(null);
    setError(null);
    invoke<SettingsSnapshot>("settings.load")
      .then((value) => {
        setSettings(value);
        onSettingsChange?.(value);
      })
      .catch((reason: Error) => setError(reason.message));
  }, [profileIndex]);
  async function update<K extends keyof SettingsSnapshot>(
    key: K,
    value: SettingsSnapshot[K],
  ) {
    setBusyKey(key);
    setError(null);
    try {
      const next = await invoke<SettingsSnapshot>("settings.update", {
        key,
        value,
      });
      setSettings(next);
      onSettingsChange?.(next);
    } catch (reason) {
      setError(
        reason instanceof Error ? reason.message : "Setting update failed",
      );
    } finally {
      setBusyKey(null);
    }
  }
  if (!settings)
    return (
      <div className="settings-page">
        <div className="feature-title">
          <div>
            <span>SYNCED PROFILE</span>
            <h1>Settings</h1>
            <p>{error || "Loading Nuvio desktop settings…"}</p>
          </div>
        </div>
      </div>
    );

  const busy = !!busyKey;
  const active = categories.find((item) => item.id === category) ?? categories[0];

  return (
    <div className="settings-page">
      <div className="feature-title">
        <div>
          <span>{active.scope === "account" ? "SYNCED PROFILE" : "THIS DESKTOP"}</span>
          <h1>Settings</h1>
          <p>{active.subtitle}</p>
          <p className={`scope-note scope-${active.scope}`}>{SCOPE_NOTE[active.scope]}</p>
        </div>
      </div>
      {error && <div className="inline-error settings-error">{error}</div>}
      <div className="settings-shell">
        <nav className="settings-nav" aria-label="Settings categories">
          {categories.map((item) => (
            <button
              key={item.id}
              className={item.id === active.id ? "active" : undefined}
              aria-current={item.id === active.id}
              onClick={() => setCategory(item.id)}
            >
              <Icon name={item.icon} size={18} />
              {item.label}
              {item.scope === "device" && (
                <i
                  className="scope-dot"
                  title="Desktop only — not shared with your phone or TV"
                />
              )}
            </button>
          ))}
        </nav>
        <div className="settings-pane">
          {active.id === "home" && (
            <HomeLayoutSection
              profileIndex={profileIndex}
              onChanged={onHomeLayoutChanged}
            />
          )}

          {active.id === "collections" && (
            <CollectionSettingsSection
              collections={collections}
              availableCatalogs={availableCatalogs}
              loading={collectionsLoading}
              error={collectionsError}
              onRefresh={onRefreshCollections}
              onReorder={onReorderCollection}
              onToggleCatalog={onToggleCatalog}
              onReorderCatalog={onReorderCatalog}
              onFolder={onOpenFolder}
            />
          )}

          {active.id === "appearance" && (
            <>
              <SettingsGroup
                title="Theme"
                subtitle="A neutral gray interface with an optional true-black canvas."
              >
                <SwitchSetting
                  label="AMOLED black"
                  detail="Use pure black surfaces for OLED displays"
                  value={settings.amoledEnabled}
                  disabled={busy}
                  onChange={(value) => update("amoledEnabled", value)}
                />
              </SettingsGroup>
              <SettingsGroup
                title="Poster card style"
                subtitle="Card width and corner radius, matching Nuvio's presets."
              >
                <PresetSetting
                  label="Width"
                  detail="Height follows at a 2:3 ratio"
                  value={settings.posterWidth}
                  options={POSTER_WIDTHS}
                  disabled={busy}
                  onChange={(value) => update("posterWidth", value)}
                />
                <PresetSetting
                  label="Corner radius"
                  detail="Roundness of poster artwork"
                  value={settings.posterCornerRadius}
                  options={POSTER_RADII}
                  disabled={busy}
                  onChange={(value) => update("posterCornerRadius", value)}
                />
                <SwitchSetting
                  label="Hide labels"
                  detail="Drop the title and year beneath each poster"
                  value={settings.posterHideLabels}
                  disabled={busy}
                  onChange={(value) => update("posterHideLabels", value)}
                />
                <SwitchSetting
                  label="Landscape posters"
                  detail="Use wide artwork in the full catalog view"
                  value={settings.posterLandscapeCatalogs}
                  disabled={busy}
                  onChange={(value) => update("posterLandscapeCatalogs", value)}
                />
              </SettingsGroup>
            </>
          )}

          {active.id === "playback" && (
            <>
              <SettingsGroup
                title="Player"
                subtitle="How video is framed and what the player shows while it works."
              >
                <SelectSetting
                  label="Video sizing"
                  detail="How video fills the window"
                  value={settings.resizeMode}
                  disabled={busy}
                  options={[
                    ["Fit", "Fit — letterbox to preserve aspect"],
                    ["Fill", "Fill — crop to fill the window"],
                    ["Zoom", "Zoom — enlarge past the edges"],
                    ["Stretch", "Stretch — ignore aspect ratio"],
                  ]}
                  onChange={(value) =>
                    update("resizeMode", value as SettingsSnapshot["resizeMode"])
                  }
                />
                <SwitchSetting
                  label="Loading overlay"
                  detail="Show status while opening a stream"
                  value={settings.showLoadingOverlay}
                  disabled={busy}
                  onChange={(value) => update("showLoadingOverlay", value)}
                />
                <SwitchSetting
                  label="Parental guide"
                  detail="Show content advisories where available"
                  value={settings.showParentalGuide}
                  disabled={busy}
                  onChange={(value) => update("showParentalGuide", value)}
                />
                <SwitchSetting
                  label="Skip intros"
                  detail="Enable supported intro-skip providers"
                  value={settings.skipIntro}
                  disabled={busy}
                  onChange={(value) => update("skipIntro", value)}
                />
                <SwitchSetting
                  label="RTX video enhancement"
                  detail="Enable NVIDIA RTX super resolution for libmpv"
                  value={settings.rtxSuperResolution}
                  disabled={busy}
                  onChange={(value) => update("rtxSuperResolution", value)}
                />
              </SettingsGroup>
              <SettingsGroup
                title="Autoplay"
                subtitle="How Nuvio picks a source and moves to the next episode."
              >
                <SelectSetting
                  label="Source selection"
                  detail="What happens when you press play"
                  value={settings.autoplayMode}
                  disabled={busy}
                  options={[
                    ["MANUAL", "Manual — always show the source list"],
                    ["FIRST_STREAM", "First stream — play the top result"],
                    ["REGEX_MATCH", "Pattern match — play the first match"],
                  ]}
                  onChange={(value) =>
                    update(
                      "autoplayMode",
                      value as SettingsSnapshot["autoplayMode"],
                    )
                  }
                />
                <SwitchSetting
                  label="Next episode"
                  detail="Automatically start the next episode"
                  value={settings.autoplayNextEpisode}
                  disabled={busy}
                  onChange={(value) => update("autoplayNextEpisode", value)}
                />
                <SwitchSetting
                  label="Reuse last stream"
                  detail="Skip the source list and replay the link you last used"
                  value={settings.reuseLastStream}
                  disabled={busy}
                  onChange={(value) => update("reuseLastStream", value)}
                />
                {settings.reuseLastStream && (
                  <PresetSetting
                    label="Keep links for"
                    detail="Links with an expiry in the URL are never reused"
                    value={settings.reuseLastStreamHours}
                    options={REUSE_WINDOWS}
                    disabled={busy}
                    onChange={(value) => update("reuseLastStreamHours", value)}
                  />
                )}
              </SettingsGroup>
            </>
          )}

          {active.id === "audio" && (
            <>
              <SettingsGroup
                title="Languages"
                subtitle="Preferred tracks to select when a stream opens."
              >
                <SelectSetting
                  label="Preferred audio"
                  value={settings.preferredAudioLanguage}
                  disabled={busy}
                  options={[
                    ["device", "Match device language"],
                    ["default", "Stream default"],
                    ["original", "Original language"],
                    ...LANGUAGES,
                  ]}
                  onChange={(value) => update("preferredAudioLanguage", value)}
                />
                <SelectSetting
                  label="Preferred subtitles"
                  value={settings.preferredSubtitleLanguage}
                  disabled={busy}
                  options={[
                    ["none", "Off"],
                    ["device", "Match device language"],
                    ["forced", "Forced subtitles only"],
                    ...LANGUAGES,
                  ]}
                  onChange={(value) =>
                    update("preferredSubtitleLanguage", value)
                  }
                />
              </SettingsGroup>
              <SettingsGroup
                title="Subtitle appearance"
                subtitle="How subtitles are drawn over video."
              >
                <NumberSetting
                  label="Subtitle size"
                  value={settings.subtitleFontSize}
                  disabled={busy}
                  onChange={(value) => update("subtitleFontSize", value)}
                />
                <SwitchSetting
                  label="Bold subtitles"
                  detail="Use heavier subtitle text"
                  value={settings.subtitleBold}
                  disabled={busy}
                  onChange={(value) => update("subtitleBold", value)}
                />
                <SwitchSetting
                  label="Subtitle outline"
                  detail="Improve readability against bright video"
                  value={settings.subtitleOutline}
                  disabled={busy}
                  onChange={(value) => update("subtitleOutline", value)}
                />
              </SettingsGroup>
            </>
          )}

          {active.id === "client" && (
            <SettingsGroup
              title="Player gestures"
              subtitle="Mouse shortcuts on the video surface. Nuvio has no equivalent for these, so they are stored here only."
            >
              <SwitchSetting
                label="Click to pause"
                detail="Left click the video to pause or resume"
                value={clientSettings.clickToPause}
                disabled={false}
                onChange={(value) => setClientSetting("clickToPause", value)}
              />
              <SwitchSetting
                label="Hold to fast-forward"
                detail="Hold right click to speed up while pressed"
                value={clientSettings.holdToSpeed}
                disabled={false}
                onChange={(value) => setClientSetting("holdToSpeed", value)}
              />
              <SwitchSetting
                label="Seek bar thumbnails"
                detail="Decode a preview frame when hovering the timeline. Opens a second connection to the source."
                value={clientSettings.seekThumbnails}
                disabled={false}
                onChange={(value) => setClientSetting("seekThumbnails", value)}
              />
              {clientSettings.holdToSpeed && (
                <PresetSetting
                  label="Hold speed"
                  detail="Playback rate while right click is held"
                  value={clientSettings.holdSpeed}
                  options={HOLD_SPEEDS}
                  disabled={false}
                  onChange={(value) => setClientSetting("holdSpeed", value)}
                />
              )}
            </SettingsGroup>
          )}

          {active.id === "streams" && (
            <>
              <SettingsGroup
                title="Source list"
                subtitle="What each stream shows in the source picker."
              >
                <SwitchSetting
                  label="File-size badges"
                  detail="Show known stream file sizes"
                  value={settings.showFileSizeBadges}
                  disabled={busy}
                  onChange={(value) => update("showFileSizeBadges", value)}
                />
                <SelectSetting
                  label="Badge placement"
                  detail="Where badges sit on a source row"
                  value={settings.badgePlacement}
                  disabled={busy}
                  options={[
                    ["TOP", "Above the title"],
                    ["BOTTOM", "Below the title"],
                  ]}
                  onChange={(value) =>
                    update(
                      "badgePlacement",
                      value as SettingsSnapshot["badgePlacement"],
                    )
                  }
                />
              </SettingsGroup>
              <SettingsGroup
                title="Notifications"
                subtitle="Alerts for series you follow."
              >
                <SwitchSetting
                  label="Episode release alerts"
                  detail="Enable release notifications for followed series"
                  value={settings.episodeReleaseAlerts}
                  disabled={busy}
                  onChange={(value) => update("episodeReleaseAlerts", value)}
                />
              </SettingsGroup>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** Named presets shown as chips, mirroring Nuvio's poster customization page.
 *  A value that matches no preset reads as "Custom", exactly as Nuvio does. */
function PresetSetting({
  label,
  detail,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  detail: string;
  value: number;
  options: readonly (readonly [string, number])[];
  disabled: boolean;
  onChange(value: number): void;
}) {
  const matched = options.some(([, preset]) => preset === value);
  return (
    <div className="setting-row preset-row">
      <div>
        <strong>{label}</strong>
        <span>
          {detail}
          {matched ? "" : ` · Custom (${value})`}
        </span>
      </div>
      <div className="preset-chips">
        {options.map(([text, preset]) => (
          <button
            key={preset}
            className={preset === value ? "active" : undefined}
            title={`${text} — ${preset}dp`}
            aria-pressed={preset === value}
            disabled={disabled}
            onClick={() => onChange(preset)}
          >
            {text}
          </button>
        ))}
      </div>
    </div>
  );
}

function SettingsGroup({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children: React.ReactNode;
}) {
  return (
    <section className="settings-group">
      <div>
        <h2>{title}</h2>
        <p>{subtitle}</p>
      </div>
      <div className="settings-list">{children}</div>
    </section>
  );
}
function SwitchSetting({
  label,
  detail,
  value,
  disabled,
  onChange,
}: {
  label: string;
  detail: string;
  value: boolean;
  disabled: boolean;
  onChange(value: boolean): void;
}) {
  return (
    <label className="setting-row">
      <div>
        <strong>{label}</strong>
        <span>{detail}</span>
      </div>
      <span className="switch">
        <input
          type="checkbox"
          checked={value}
          disabled={disabled}
          onChange={(event) => onChange(event.target.checked)}
        />
        <i />
      </span>
    </label>
  );
}
/** Options are `[storedValue, humanLabel]` — the stored half must stay exactly
 *  what Nuvio's settings blob expects. */
function SelectSetting({
  label,
  detail,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  detail?: string;
  value: string;
  options: readonly (readonly [string, string])[];
  disabled: boolean;
  onChange(value: string): void;
}) {
  return (
    <label className="setting-row">
      <div>
        <strong>{label}</strong>
        {detail && <span>{detail}</span>}
      </div>
      <select
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map(([option, text]) => (
          <option key={option} value={option}>
            {text}
          </option>
        ))}
      </select>
    </label>
  );
}
function NumberSetting({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value: number;
  disabled: boolean;
  onChange(value: number): void;
}) {
  return (
    <label className="setting-row">
      <div>
        <strong>{label}</strong>
        <span>12–40 px</span>
      </div>
      <input
        className="number-input"
        type="number"
        min={12}
        max={40}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </label>
  );
}

import { useEffect, useMemo, useRef, useState } from "react";
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
import { UpdatesSection } from "./UpdatesSection";
import { DownloadSettingsSection } from "./DownloadSettingsSection";
import { Icon } from "./Icon";
import { setClientSetting, useClientSettings } from "../data/clientSettings";
import type { ClientSettings } from "../data/clientSettings";
import {
  SECTIONS,
  searchSettings,
  type SettingDef,
  type SettingScope,
} from "../data/settingsRegistry";

const SCOPE_NOTE: Record<SettingScope, string> = {
  account: "Shared with every device on this profile — phone, TV and desktop.",
  device:
    "Stored per platform. Nuvio keeps separate desktop and mobile settings, so these do not reach your phone or TV.",
  local:
    "Not synced anywhere. These exist only in this client and are stored on this machine.",
};

export function SettingsPage({
  profileIndex,
  settings,
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
  settings: SettingsSnapshot | null;
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
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sectionId, setSectionId] = useState<string>(SECTIONS[0].id);
  const [query, setQuery] = useState("");
  const [highlight, setHighlight] = useState<string | null>(null);
  const client = useClientSettings();
  const paneRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setError(null);
  }, [profileIndex]);

  // Highlighting is a one-shot cue after jumping from a search result, so it
  // clears itself rather than leaving a row marked while you work.
  useEffect(() => {
    if (!highlight) return;
    paneRef.current
      ?.querySelector(`[data-setting="${highlight}"]`)
      ?.scrollIntoView({ block: "center", behavior: "smooth" });
    const timer = window.setTimeout(() => setHighlight(null), 2400);
    return () => window.clearTimeout(timer);
  }, [highlight, sectionId]);

  const hits = useMemo(() => searchSettings(query), [query]);

  async function updateSynced(key: string, value: unknown) {
    setBusyKey(key);
    setError(null);
    try {
      const next = await invoke<SettingsSnapshot>("settings.update", { key, value });
      onSettingsChange?.(next);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Setting update failed");
    } finally {
      setBusyKey(null);
    }
  }

  if (!settings)
    return (
      <div className="settings-page">
        <div className="feature-title">
          <div>
            <h1>Settings</h1>
            <p>{error || "Settings are unavailable for this profile."}</p>
          </div>
        </div>
      </div>
    );

  const section = SECTIONS.find((item) => item.id === sectionId) ?? SECTIONS[0];

  const readValue = (setting: SettingDef, scope: SettingScope): unknown =>
    scope === "local"
      ? client[setting.id as keyof ClientSettings]
      : settings[setting.id as keyof SettingsSnapshot];

  const isVisible = (setting: SettingDef, scope: SettingScope) => {
    if (!setting.requires) return true;
    return scope === "local"
      ? !!client[setting.requires as keyof ClientSettings]
      : !!settings[setting.requires as keyof SettingsSnapshot];
  };

  const commit = (setting: SettingDef, scope: SettingScope, value: unknown) => {
    if (scope === "local") {
      setClientSetting(setting.id as keyof ClientSettings, value as never);
      return;
    }
    void updateSynced(String(setting.id), value);
  };

  return (
    <div className="settings-page">
      <div className="feature-title">
        <div>
          <h1>Settings</h1>
          <p>{section.subtitle}</p>
          <p className={`scope-note scope-${section.scope}`}>
            {SCOPE_NOTE[section.scope]}
          </p>
        </div>
      </div>
      {error && <div className="inline-error settings-error">{error}</div>}

      <div className="settings-shell">
        <div className="settings-side">
          <label className="settings-search">
            <Icon name="search" size={17} />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search settings…"
              aria-label="Search settings"
            />
            {query && (
              <button
                className="search-clear"
                title="Clear"
                aria-label="Clear search"
                onClick={() => setQuery("")}
              >
                <Icon name="close" size={15} />
              </button>
            )}
          </label>
          <nav className="settings-nav" aria-label="Settings sections">
            {SECTIONS.map((item) => (
              <button
                key={item.id}
                className={item.id === section.id && !query ? "active" : undefined}
                aria-current={item.id === section.id}
                onClick={() => {
                  setQuery("");
                  setSectionId(item.id);
                }}
              >
                <Icon name={item.icon as never} size={18} />
                {item.label}
                {item.scope !== "account" && (
                  <i
                    className="scope-dot"
                    title={
                      item.scope === "local"
                        ? "Not synced — this machine only"
                        : "Desktop only — not shared with your phone or TV"
                    }
                  />
                )}
              </button>
            ))}
          </nav>
        </div>

        <div className="settings-pane" ref={paneRef}>
          {query ? (
            <section className="settings-group">
              <div>
                <h2>
                  {hits.length} result{hits.length === 1 ? "" : "s"}
                </h2>
                <p>Matching settings across every section.</p>
              </div>
              <div className="settings-list">
                {hits.length === 0 ? (
                  <div className="collection-settings-empty">
                    Nothing matches “{query}”.
                  </div>
                ) : (
                  hits.map(({ section: hitSection, group, setting }) => (
                    <button
                      className="settings-hit"
                      key={`${hitSection.id}:${String(setting.id)}`}
                      onClick={() => {
                        setQuery("");
                        setSectionId(hitSection.id);
                        setHighlight(String(setting.id));
                      }}
                    >
                      <div>
                        <strong>{setting.label}</strong>
                        {setting.detail && <span>{setting.detail}</span>}
                      </div>
                      <em>
                        {hitSection.label} › {group.title}
                      </em>
                    </button>
                  ))
                )}
              </div>
            </section>
          ) : section.custom === "homeLayout" ? (
            <HomeLayoutSection
              profileIndex={profileIndex}
              onChanged={onHomeLayoutChanged}
            />
          ) : section.custom === "collections" ? (
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
          ) : section.custom === "updates" ? (
            <UpdatesSection />
          ) : section.custom === "downloads" ? (
            <DownloadSettingsSection />
          ) : (
            section.groups.map((group) => (
              <section className="settings-group" key={group.title}>
                <div>
                  <h2>{group.title}</h2>
                  <p>{group.subtitle}</p>
                </div>
                <div className="settings-list">
                  {group.settings
                    .filter((setting) => isVisible(setting, section.scope))
                    .map((setting) => (
                      <SettingRow
                        key={String(setting.id)}
                        setting={setting}
                        value={readValue(setting, section.scope)}
                        busy={!!busyKey}
                        highlighted={highlight === String(setting.id)}
                        onChange={(value) => commit(setting, section.scope, value)}
                      />
                    ))}
                </div>
              </section>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

const CHIP_LIMIT = 6;

function SettingRow({
  setting,
  value,
  busy,
  highlighted,
  onChange,
}: {
  setting: SettingDef;
  value: unknown;
  busy: boolean;
  highlighted: boolean;
  onChange(value: unknown): void;
}) {
  const control = setting.control;
  return (
    <div
      className={highlighted ? "setting-row is-highlighted" : "setting-row"}
      data-setting={String(setting.id)}
    >
      <div>
        <strong>{setting.label}</strong>
        {setting.detail && <span>{setting.detail}</span>}
      </div>

      {control.kind === "switch" && (
        <label className="switch">
          <input
            type="checkbox"
            aria-label={setting.label}
            checked={!!value}
            disabled={busy}
            onChange={(event) => onChange(event.target.checked)}
          />
          <i />
        </label>
      )}

      {control.kind === "preset" &&
        // Chips only read well while they fit on one line. Longer sets (the
        // language lists) become a dropdown rather than wrapping to two rows.
        (control.options.length > CHIP_LIMIT ? (
          <select
            className="setting-select"
            aria-label={setting.label}
            value={String(value ?? "")}
            disabled={busy}
            onChange={(event) => {
              const picked = control.options.find(
                ([, option]) => String(option) === event.target.value,
              );
              if (picked) onChange(picked[1]);
            }}
          >
            {control.options.map(([label, option]) => (
              <option key={String(option)} value={String(option)}>
                {label}
              </option>
            ))}
          </select>
        ) : (
          <div className="preset-chips">
            {control.options.map(([label, option]) => (
              <button
                key={String(option)}
                className={option === value ? "active" : undefined}
                aria-pressed={option === value}
                disabled={busy}
                onClick={() => onChange(option)}
              >
                {label}
              </button>
            ))}
          </div>
        ))}

      {control.kind === "number" && (
        <div className="poster-size-control">
          <input
            type="range"
            aria-label={setting.label}
            min={control.min}
            max={control.max}
            step={control.step ?? 1}
            value={Number(value) || control.min}
            disabled={busy}
            onChange={(event) => onChange(Number(event.target.value))}
          />
          <output>
            {Number(value) || control.min}
            {control.suffix ?? ""}
          </output>
        </div>
      )}

      {control.kind === "text" && (
        <SettingTextField
          value={String(value ?? "")}
          placeholder={control.placeholder}
          secret={control.secret}
          disabled={busy}
          onCommit={onChange}
        />
      )}

      {control.kind === "color" && (
        <ColorField
          value={String(value ?? "#FFFFFFFF")}
          disabled={busy}
          onCommit={onChange}
        />
      )}
    </div>
  );
}

/** Commits on blur or Enter, so a key or pattern is not pushed per keystroke. */
function SettingTextField({
  value,
  placeholder,
  secret,
  disabled,
  onCommit,
}: {
  value: string;
  placeholder?: string;
  secret?: boolean;
  disabled: boolean;
  onCommit(value: string): void;
}) {
  const [draft, setDraft] = useState(value);
  const [revealed, setRevealed] = useState(false);
  useEffect(() => setDraft(value), [value]);
  return (
    <div className="setting-text-field">
      <input
        type={secret && !revealed ? "password" : "text"}
        value={draft}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => draft !== value && onCommit(draft)}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
          if (event.key === "Escape") setDraft(value);
        }}
      />
      {secret && (
        <button
          type="button"
          className="icon-action"
          title={revealed ? "Hide" : "Reveal"}
          aria-label={revealed ? "Hide" : "Reveal"}
          onClick={() => setRevealed(!revealed)}
        >
          <Icon name="eye" size={16} />
        </button>
      )}
    </div>
  );
}

/**
 * Nuvio stores subtitle colours as #AARRGGBB, but the native picker only
 * speaks #RRGGBB — so alpha rides a separate slider and is recombined here.
 */
function ColorField({
  value,
  disabled,
  onCommit,
}: {
  value: string;
  disabled: boolean;
  onCommit(value: string): void;
}) {
  const body = value.replace("#", "");
  const alpha = body.length === 8 ? body.slice(0, 2) : "FF";
  const rgb = (body.length === 8 ? body.slice(2) : body).padEnd(6, "0").slice(0, 6);
  const alphaPercent = Math.round((parseInt(alpha, 16) / 255) * 100);

  return (
    <div className="setting-color-field">
      <input
        type="color"
        aria-label="Colour"
        value={`#${rgb}`}
        disabled={disabled}
        onChange={(event) =>
          onCommit(`#${alpha}${event.target.value.replace("#", "").toUpperCase()}`)
        }
      />
      <input
        type="range"
        aria-label="Opacity"
        min={0}
        max={100}
        value={alphaPercent}
        disabled={disabled}
        onChange={(event) => {
          const next = Math.round((Number(event.target.value) / 100) * 255)
            .toString(16)
            .padStart(2, "0")
            .toUpperCase();
          onCommit(`#${next}${rgb}`);
        }}
      />
      <output>{alphaPercent}%</output>
    </div>
  );
}

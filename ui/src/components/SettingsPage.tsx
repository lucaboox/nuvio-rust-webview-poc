import { useEffect, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { CollectionFolder, NuvioCollection, SettingsSnapshot } from "../bridge/types";
import { CollectionSettingsSection } from "./CollectionsPage";

export function SettingsPage({ profileIndex, collections, collectionsLoading, collectionsError, onRefreshCollections, onReorderCollection, onOpenFolder, onThemeChange }: { profileIndex: number; collections: NuvioCollection[]; collectionsLoading: boolean; collectionsError: string | null; onRefreshCollections(): void; onReorderCollection(collectionId: string, folderId: string | undefined, direction: -1 | 1): void; onOpenFolder(collection: NuvioCollection, folder: CollectionFolder): void; onThemeChange?(amoled: boolean): void }) {
  const [settings, setSettings] = useState<SettingsSnapshot | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => { setSettings(null); setError(null); invoke<SettingsSnapshot>("settings.load").then((value) => { setSettings(value); onThemeChange?.(value.amoledEnabled); }).catch((reason: Error) => setError(reason.message)); }, [profileIndex]);
  async function update<K extends keyof SettingsSnapshot>(key: K, value: SettingsSnapshot[K]) {
    setBusyKey(key); setError(null);
    try { const next = await invoke<SettingsSnapshot>("settings.update", { key, value }); setSettings(next); if (key === "amoledEnabled") onThemeChange?.(next.amoledEnabled); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "Setting update failed"); }
    finally { setBusyKey(null); }
  }
  if (!settings) return <div className="settings-page"><div className="feature-title"><div><span>SYNCED PROFILE</span><h1>Settings</h1><p>{error || "Loading Nuvio desktop settings…"}</p></div></div></div>;

  return <div className="settings-page"><div className="feature-title"><div><span>SYNCED PROFILE</span><h1>Settings</h1><p>Playback and content options use Nuvio's versioned desktop settings blob.</p></div></div>{error && <div className="inline-error settings-error">{error}</div>}
    <CollectionSettingsSection collections={collections} loading={collectionsLoading} error={collectionsError} onRefresh={onRefreshCollections} onReorder={onReorderCollection} onFolder={onOpenFolder} />
    <SettingsGroup title="Appearance" subtitle="A neutral gray interface with an optional true-black canvas">
      <SwitchSetting label="AMOLED black" detail="Use pure black surfaces for OLED displays" value={settings.amoledEnabled} disabled={!!busyKey} onChange={(value) => update("amoledEnabled", value)} />
    </SettingsGroup>
    <SettingsGroup title="Playback" subtitle="Player behavior and native playback defaults">
      <SelectSetting label="Video sizing" value={settings.resizeMode} disabled={busyKey === "resizeMode"} options={["Fit", "Fill", "Zoom", "Stretch"]} onChange={(value) => update("resizeMode", value as SettingsSnapshot["resizeMode"])} />
      <SwitchSetting label="Loading overlay" detail="Show status while opening a stream" value={settings.showLoadingOverlay} disabled={!!busyKey} onChange={(value) => update("showLoadingOverlay", value)} />
      <SwitchSetting label="Parental guide" detail="Show content advisories where available" value={settings.showParentalGuide} disabled={!!busyKey} onChange={(value) => update("showParentalGuide", value)} />
      <SwitchSetting label="Reuse last stream" detail="Reuse a recent working source when possible" value={settings.reuseLastStream} disabled={!!busyKey} onChange={(value) => update("reuseLastStream", value)} />
      <SwitchSetting label="Skip intros" detail="Enable supported intro-skip providers" value={settings.skipIntro} disabled={!!busyKey} onChange={(value) => update("skipIntro", value)} />
      <SwitchSetting label="RTX video enhancement" detail="Enable NVIDIA RTX super resolution for libmpv" value={settings.rtxSuperResolution} disabled={!!busyKey} onChange={(value) => update("rtxSuperResolution", value)} />
    </SettingsGroup>
    <SettingsGroup title="Autoplay" subtitle="How Nuvio chooses and advances streams">
      <SelectSetting label="Source selection" value={settings.autoplayMode} disabled={!!busyKey} options={["MANUAL", "FIRST_STREAM", "REGEX_MATCH"]} onChange={(value) => update("autoplayMode", value as SettingsSnapshot["autoplayMode"])} />
      <SwitchSetting label="Next episode" detail="Automatically start the next episode" value={settings.autoplayNextEpisode} disabled={!!busyKey} onChange={(value) => update("autoplayNextEpisode", value)} />
    </SettingsGroup>
    <SettingsGroup title="Audio and subtitles" subtitle="Language selection and subtitle rendering">
      <SelectSetting label="Preferred audio" value={settings.preferredAudioLanguage} disabled={!!busyKey} options={["device", "default", "original", "en", "es", "fr", "de", "ja"]} onChange={(value) => update("preferredAudioLanguage", value)} />
      <SelectSetting label="Preferred subtitles" value={settings.preferredSubtitleLanguage} disabled={!!busyKey} options={["none", "device", "forced", "en", "es", "fr", "de", "ja"]} onChange={(value) => update("preferredSubtitleLanguage", value)} />
      <NumberSetting label="Subtitle size" value={settings.subtitleFontSize} disabled={!!busyKey} onChange={(value) => update("subtitleFontSize", value)} />
      <SwitchSetting label="Bold subtitles" detail="Use heavier subtitle text" value={settings.subtitleBold} disabled={!!busyKey} onChange={(value) => update("subtitleBold", value)} />
      <SwitchSetting label="Subtitle outline" detail="Improve readability against bright video" value={settings.subtitleOutline} disabled={!!busyKey} onChange={(value) => update("subtitleOutline", value)} />
    </SettingsGroup>
    <SettingsGroup title="Streams and notifications" subtitle="Source presentation and episode alerts">
      <SwitchSetting label="File-size badges" detail="Show known stream file sizes" value={settings.showFileSizeBadges} disabled={!!busyKey} onChange={(value) => update("showFileSizeBadges", value)} />
      <SelectSetting label="Badge placement" value={settings.badgePlacement} disabled={!!busyKey} options={["TOP", "BOTTOM"]} onChange={(value) => update("badgePlacement", value as SettingsSnapshot["badgePlacement"])} />
      <SwitchSetting label="Episode release alerts" detail="Enable release notifications for followed series" value={settings.episodeReleaseAlerts} disabled={!!busyKey} onChange={(value) => update("episodeReleaseAlerts", value)} />
    </SettingsGroup>
  </div>;
}

function SettingsGroup({ title, subtitle, children }: { title: string; subtitle: string; children: React.ReactNode }) { return <section className="settings-group"><div><h2>{title}</h2><p>{subtitle}</p></div><div className="settings-list">{children}</div></section>; }
function SwitchSetting({ label, detail, value, disabled, onChange }: { label: string; detail: string; value: boolean; disabled: boolean; onChange(value: boolean): void }) { return <label className="setting-row"><div><strong>{label}</strong><span>{detail}</span></div><span className="switch"><input type="checkbox" checked={value} disabled={disabled} onChange={(event) => onChange(event.target.checked)} /><i /></span></label>; }
function SelectSetting({ label, value, options, disabled, onChange }: { label: string; value: string; options: string[]; disabled: boolean; onChange(value: string): void }) { return <label className="setting-row"><div><strong>{label}</strong></div><select value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)}>{options.map((option) => <option key={option}>{option}</option>)}</select></label>; }
function NumberSetting({ label, value, disabled, onChange }: { label: string; value: number; disabled: boolean; onChange(value: number): void }) { return <label className="setting-row"><div><strong>{label}</strong><span>12–40 px</span></div><input className="number-input" type="number" min={12} max={40} value={value} disabled={disabled} onChange={(event) => onChange(Number(event.target.value))} /></label>; }

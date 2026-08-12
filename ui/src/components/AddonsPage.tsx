import { FormEvent, useEffect, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { AccountPayload, AddonDescriptor, AddonRow } from "../bridge/types";

export function AddonsPage({ addons, loading, onAccount, onRefresh }: { addons: AddonRow[]; loading: boolean; onAccount(payload: AccountPayload): void; onRefresh(): void }) {
  const [descriptors, setDescriptors] = useState<AddonDescriptor[]>([]);
  const [canEdit, setCanEdit] = useState(false);
  const [busy, setBusy] = useState(false);
  const [url, setUrl] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const signature = addons.map((addon) => `${addon.url}:${addon.enabled}:${addon.sortOrder}`).join("|");

  useEffect(() => {
    invoke<{ addons: AddonDescriptor[]; canEdit: boolean }>("addons.describe")
      .then((result) => { setDescriptors(result.addons); setCanEdit(result.canEdit); })
      .catch((error: Error) => setMessage(error.message));
  }, [signature]);

  async function mutate(method: string, params: unknown) {
    setBusy(true); setMessage(null);
    try { onAccount(await invoke<AccountPayload>(method, params)); }
    catch (error) { setMessage(error instanceof Error ? error.message : "Addon update failed"); }
    finally { setBusy(false); }
  }

  async function add(event: FormEvent) {
    event.preventDefault(); if (!url.trim()) return;
    await mutate("addons.add", { url: url.trim() }); setUrl("");
  }

  async function configure(addon: AddonDescriptor) {
    if (!addon.configureUrl) return;
    try { await invoke("system.openExternal", { url: addon.configureUrl }); }
    catch (error) { setMessage(error instanceof Error ? error.message : "Could not open addon configuration"); }
  }

  async function refreshOne(addon: AddonDescriptor) {
    setBusy(true); setMessage(null);
    try {
      const refreshed = await invoke<AddonDescriptor>("addons.refreshOne", { url: addon.url });
      setDescriptors((current) => current.map((item) => item.url === addon.url ? refreshed : item));
    } catch (error) { setMessage(error instanceof Error ? error.message : "Addon refresh failed"); }
    finally { setBusy(false); }
  }

  return <div className="feature-page">
    <div className="feature-title"><div><span>STREMIO ADDONS</span><h1>Your addons</h1><p>Changes sync through Nuvio's addon endpoint for this profile.</p></div><button onClick={onRefresh} disabled={loading || busy}>{loading ? "Refreshing…" : "Refresh"}</button></div>
    {canEdit && <form className="addon-add" onSubmit={add}><input value={url} onChange={(event) => setUrl(event.target.value)} placeholder="Paste addon URL or manifest URL" /><button disabled={busy}>Install addon</button></form>}
    {!canEdit && <div className="notice-banner addon-notice"><span>This profile inherits addons from the primary profile. Switch profiles to edit them.</span></div>}
    {message && <div className="inline-error addon-error">{message}</div>}
    {descriptors.length === 0 ? <div className="empty-feature"><strong>No synced addons for this profile</strong><span>Install a Stremio addon to populate Home, search, metadata and streams.</span></div> : <div className="addon-list">{descriptors.map((addon, index) => {
      const host = (() => { try { return new URL(addon.url).host; } catch { return addon.url; } })();
      return <article className="addon-row addon-managed" key={addon.url}>
        <div className="addon-icon">{addon.logo ? <img src={addon.logo} alt="" /> : (addon.name || host).slice(0, 1).toUpperCase()}</div>
        <div><strong>{addon.name}</strong><span>{host} · {addon.catalogCount} catalogs · {addon.resourceNames.join(", ") || "manifest"}</span>{addon.error && <small>{addon.error}</small>}</div>
        <div className="addon-actions">
          <button disabled={busy} onClick={() => refreshOne(addon)}>Refresh</button>
          {addon.configureUrl && <button onClick={() => configure(addon)}>Configure</button>}
          <label className="switch"><input type="checkbox" checked={addon.enabled} disabled={!canEdit || busy} onChange={(event) => mutate("addons.toggle", { url: addon.url, enabled: event.target.checked })} /><i /></label>
          <button disabled={!canEdit || busy || index === 0} onClick={() => mutate("addons.move", { from: index, to: index - 1 })}>↑</button><button disabled={!canEdit || busy || index === descriptors.length - 1} onClick={() => mutate("addons.move", { from: index, to: index + 1 })}>↓</button>
          <button className="danger-action" disabled={!canEdit || busy} onClick={() => confirm(`Remove ${addon.name}?`) && mutate("addons.remove", { url: addon.url })}>Remove</button>
        </div>
      </article>;
    })}</div>}
  </div>;
}

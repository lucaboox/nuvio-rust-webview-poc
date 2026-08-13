import { FormEvent, useEffect, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { AccountPayload, AddonDescriptor, AddonRow } from "../bridge/types";
import { Icon } from "./Icon";

export function AddonsPage({
  addons,
  loading,
  onAccount,
  onRefresh,
}: {
  addons: AddonRow[];
  loading: boolean;
  onAccount(payload: AccountPayload): void;
  onRefresh(): void;
}) {
  const [descriptors, setDescriptors] = useState<AddonDescriptor[]>([]);
  const [canEdit, setCanEdit] = useState(false);
  const [busy, setBusy] = useState(false);
  const [refreshingUrl, setRefreshingUrl] = useState<string | null>(null);
  const [url, setUrl] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const signature = addons
    .map((addon) => `${addon.url}:${addon.enabled}:${addon.sortOrder}`)
    .join("|");

  useEffect(() => {
    invoke<{ addons: AddonDescriptor[]; canEdit: boolean }>("addons.describe")
      .then((result) => {
        setDescriptors(result.addons);
        setCanEdit(result.canEdit);
      })
      .catch((error: Error) => setMessage(error.message));
  }, [signature]);

  async function mutate(method: string, params: unknown) {
    setBusy(true);
    setMessage(null);
    try {
      onAccount(await invoke<AccountPayload>(method, params));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Addon update failed");
    } finally {
      setBusy(false);
    }
  }

  async function add(event: FormEvent) {
    event.preventDefault();
    if (!url.trim()) return;
    await mutate("addons.add", { url: url.trim() });
    setUrl("");
  }

  async function configure(addon: AddonDescriptor) {
    if (!addon.configureUrl) return;
    try {
      await invoke("system.openExternal", { url: addon.configureUrl });
    } catch (error) {
      setMessage(
        error instanceof Error
          ? error.message
          : "Could not open addon configuration",
      );
    }
  }

  async function refreshOne(addon: AddonDescriptor) {
    setRefreshingUrl(addon.url);
    setMessage(null);
    try {
      const refreshed = await invoke<AddonDescriptor>("addons.refreshOne", {
        url: addon.url,
      });
      setDescriptors((current) =>
        current.map((item) => (item.url === addon.url ? refreshed : item)),
      );
    } catch (error) {
      setMessage(
        error instanceof Error ? error.message : "Addon refresh failed",
      );
    } finally {
      setRefreshingUrl(null);
    }
  }

  const enabledCount = descriptors.filter((addon) => addon.enabled).length;

  return (
    <div className="feature-page addons-page">
      <div className="feature-title">
        <div>
          <span>STREMIO ADDONS</span>
          <h1>Your addons</h1>
          <p>
            {descriptors.length
              ? `${enabledCount} of ${descriptors.length} enabled · changes sync to this profile`
              : "Changes sync through Nuvio's addon endpoint for this profile."}
          </p>
          {descriptors.length > 1 && (
            <p className="addon-order-hint">
              Order is priority. Nuvio walks this list top-down and takes the
              first addon that answers, so the top one decides posters,
              descriptions and episode lists. It also sets the order of stream
              sources and search results.
            </p>
          )}
        </div>
        <button
          className="icon-action addons-refresh"
          title="Reload addons from your account"
          onClick={onRefresh}
          disabled={loading || busy}
        >
          <Icon name="refresh" size={22} />
        </button>
      </div>

      {canEdit && (
        <form className="addon-add" onSubmit={add}>
          <Icon name="plus" size={18} />
          <input
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            placeholder="Paste an addon or manifest URL"
            aria-label="Addon URL"
          />
          <button
            className="icon-action primary"
            title="Install this addon"
            disabled={busy || !url.trim()}
          >
            <Icon name="check" size={19} />
          </button>
        </form>
      )}
      {!canEdit && (
        <div className="notice-banner addon-notice">
          <span>
            This profile inherits addons from the primary profile. Switch
            profiles to edit them.
          </span>
        </div>
      )}
      {message && <div className="inline-error addon-error">{message}</div>}

      {descriptors.length === 0 ? (
        <div className="empty-feature">
          <strong>No synced addons for this profile</strong>
          <span>
            Install a Stremio addon to populate Home, search, metadata and
            streams.
          </span>
        </div>
      ) : (
        <div className="addon-list">
          {descriptors.map((addon, index) => {
            const host = (() => {
              try {
                return new URL(addon.url).host;
              } catch {
                return addon.url;
              }
            })();
            const refreshing = refreshingUrl === addon.url;
            return (
              <article
                className={
                  addon.enabled ? "addon-card" : "addon-card is-disabled"
                }
                key={addon.url}
              >
                <div className="addon-icon">
                  {addon.logo ? (
                    <img src={addon.logo} alt="" />
                  ) : (
                    (addon.name || host).slice(0, 1).toUpperCase()
                  )}
                </div>
                <div className="addon-copy">
                  <div className="addon-name">
                    <strong title={addon.name}>{addon.name}</strong>
                    {addon.version && (
                      <code title="Manifest version, read live from the addon">
                        v{addon.version}
                      </code>
                    )}
                  </div>
                  <span title={addon.url}>{host}</span>
                  <div className="addon-badges">
                    <em title={`${addon.catalogCount} home catalogs`}>
                      {addon.catalogCount} catalog
                      {addon.catalogCount === 1 ? "" : "s"}
                    </em>
                    {addon.resourceNames.slice(0, 3).map((resource) => (
                      <em key={resource}>{resource}</em>
                    ))}
                    {!addon.enabled && <em className="muted">Disabled</em>}
                    {addon.error && (
                      <em className="bad" title={addon.error}>
                        Unreachable
                      </em>
                    )}
                  </div>
                </div>
                <div className="addon-actions">
                  <label
                    className="switch"
                    title={
                      addon.enabled ? "Disable this addon" : "Enable this addon"
                    }
                  >
                    <input
                      type="checkbox"
                      aria-label={`Enable ${addon.name}`}
                      checked={addon.enabled}
                      disabled={!canEdit || busy}
                      onChange={(event) =>
                        mutate("addons.toggle", {
                          url: addon.url,
                          enabled: event.target.checked,
                        })
                      }
                    />
                    <i />
                  </label>
                  <button
                    className="icon-action"
                    title="Reload this addon's manifest"
                    disabled={busy || refreshing}
                    onClick={() => refreshOne(addon)}
                  >
                    <Icon name="refresh" size={17} />
                  </button>
                  {addon.configureUrl && (
                    <button
                      className="icon-action"
                      title="Open this addon's configuration page"
                      onClick={() => configure(addon)}
                    >
                      <Icon name="external" size={17} />
                    </button>
                  )}
                  <button
                    className="icon-action"
                    title="Raise priority — the first addon that answers provides the metadata"
                    disabled={!canEdit || busy || index === 0}
                    onClick={() =>
                      mutate("addons.move", { from: index, to: index - 1 })
                    }
                  >
                    <Icon name="up" size={17} />
                  </button>
                  <button
                    className="icon-action"
                    title="Lower priority"
                    disabled={
                      !canEdit || busy || index === descriptors.length - 1
                    }
                    onClick={() =>
                      mutate("addons.move", { from: index, to: index + 1 })
                    }
                  >
                    <Icon name="down" size={17} />
                  </button>
                  <button
                    className="icon-action danger"
                    title="Remove this addon"
                    disabled={!canEdit || busy}
                    onClick={() =>
                      confirm(`Remove ${addon.name}?`) &&
                      mutate("addons.remove", { url: addon.url })
                    }
                  >
                    <Icon name="trash" size={17} />
                  </button>
                </div>
              </article>
            );
          })}
        </div>
      )}
    </div>
  );
}

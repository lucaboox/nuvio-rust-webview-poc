import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type { HomeLayoutAction, HomeLayoutState } from "../bridge/types";
import { Icon } from "./Icon";

/**
 * The Home Layout organizer — the same ordered list of catalogs and collections
 * Nuvio shows under Settings › Home Layout, backed by the same synced payload.
 *
 * Hero preferences are deliberately device-local: Nuvio never puts them on the
 * wire, so toggling one here does not touch the phone.
 */
export function HomeLayoutSection({
  profileIndex,
  onChanged,
}: {
  profileIndex: number;
  onChanged?(): void;
}) {
  const [layout, setLayout] = useState<HomeLayoutState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragKey, setDragKey] = useState<string | null>(null);
  const [dropIndex, setDropIndex] = useState<number | null>(null);
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [titleDraft, setTitleDraft] = useState("");
  const [heroPickerOpen, setHeroPickerOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLayout(null);
    setError(null);
    setBusy(true);
    invoke<HomeLayoutState>("homeLayout.list")
      .then((value) => {
        if (!cancelled) setLayout(value);
      })
      .catch((reason: Error) => {
        if (!cancelled) setError(reason.message);
      })
      .finally(() => {
        if (!cancelled) setBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [profileIndex]);

  async function update(params: HomeLayoutAction) {
    setBusy(true);
    setError(null);
    try {
      setLayout(await invoke<HomeLayoutState>("homeLayout.update", params));
      onChanged?.();
    } catch (reason) {
      setError(
        reason instanceof Error
          ? reason.message
          : "Could not update the home layout",
      );
    } finally {
      setBusy(false);
    }
  }

  const items = layout?.items ?? [];
  const catalogItems = useMemo(
    () => items.filter((item) => !item.isCollection),
    [items],
  );
  const heroSourceCount = catalogItems.filter(
    (item) => item.heroSourceEnabled,
  ).length;
  const visibleCount = items.filter((item) => item.enabled).length;
  const collectionCount = items.filter((item) => item.isCollection).length;
  const sectionTitle =
    collectionCount > 0 && catalogItems.length > 0
      ? "Catalogs & Collections"
      : collectionCount > 0
        ? "Collections"
        : "Catalogs";

  function commitDrop(toIndex: number) {
    const fromIndex = items.findIndex((item) => item.key === dragKey);
    setDragKey(null);
    setDropIndex(null);
    if (fromIndex < 0 || fromIndex === toIndex) return;
    const item = items[fromIndex];
    if (item.pinnedToTop) {
      setError("Pinned collections always stay at the top of Home.");
      return;
    }
    void update({ action: "move", from: fromIndex, to: toIndex });
  }

  return (
    <section className="settings-group home-layout-group">
      <div>
        <h2>Home Layout</h2>
        <p>
          Reorder, rename and hide the catalogs and collections on Home. Order,
          visibility and custom titles sync with Nuvio; hero choices stay on this
          device.
        </p>
        {layout && (
          <span className="home-layout-summary">
            {visibleCount} of {items.length} visible • {heroSourceCount} hero
            source{heroSourceCount === 1 ? "" : "s"}
          </span>
        )}
        {!!layout?.preservedCount && (
          <span className="home-layout-summary">
            {layout.preservedCount} row{layout.preservedCount === 1 ? "" : "s"}{" "}
            from your other devices kept as-is
          </span>
        )}
      </div>
      <div className="home-layout-body">
        {error && <div className="inline-error">{error}</div>}
        {!layout ? (
          <div className="library-loading">
            <i className="loading-spinner" /> Loading home layout…
          </div>
        ) : (
          <>
            <div className="settings-list">
              <ToggleRow
                title="Show Hero Section"
                detail="Display the hero carousel at the top of Home. This device only."
                checked={layout.heroEnabled}
                busy={busy}
                onChange={(enabled) =>
                  update({ action: "setHeroEnabled", enabled })
                }
              />
              <ToggleRow
                title="Show Catalog Type"
                detail="Show the type suffix next to catalog names (Movies/Series)."
                checked={layout.showCatalogType}
                busy={busy}
                onChange={(enabled) =>
                  update({ action: "setShowCatalogType", enabled })
                }
              />
              <ToggleRow
                title="Hide Unreleased Content"
                detail="Hide movies and shows that haven't been released yet."
                checked={layout.hideUnreleasedContent}
                busy={busy}
                onChange={(enabled) =>
                  update({ action: "setHideUnreleasedContent", enabled })
                }
              />
            </div>

            {layout.heroEnabled && catalogItems.length > 0 && (
              <div className="home-layout-hero">
                <h3>Hero Catalogs</h3>
                <button
                  className="hero-picker-toggle"
                  aria-expanded={heroPickerOpen}
                  onClick={() => setHeroPickerOpen((open) => !open)}
                >
                  <span>
                    <strong>
                      {heroSourceCount} of {layout.heroSourceLimit} selected
                    </strong>
                    <em>
                      {catalogItems
                        .filter((item) => item.heroSourceEnabled)
                        .map((item) => item.displayTitle)
                        .join(", ") || "No hero sources selected"}
                    </em>
                  </span>
                  <Icon name={heroPickerOpen ? "up" : "down"} size={18} />
                </button>
                {heroPickerOpen && (
                  <div className="hero-picker-list">
                    {catalogItems.map((item) => {
                      const atLimit =
                        !item.heroSourceEnabled &&
                        heroSourceCount >= layout.heroSourceLimit;
                      return (
                        <label
                          key={item.key}
                          className={atLimit ? "disabled" : undefined}
                        >
                          <input
                            type="checkbox"
                            checked={item.heroSourceEnabled}
                            disabled={busy || atLimit}
                            onChange={(event) =>
                              update({
                                action: "setHeroSourceEnabled",
                                key: item.key,
                                enabled: event.target.checked,
                              })
                            }
                          />
                          <span>
                            <strong>{item.displayTitle}</strong>
                            <em>
                              {atLimit
                                ? `Only ${layout.heroSourceLimit} hero catalogs can be selected`
                                : item.subtitle}
                            </em>
                          </span>
                        </label>
                      );
                    })}
                  </div>
                )}
              </div>
            )}

            <div className="home-layout-header">
              <h3>{sectionTitle}</h3>
              <button
                className="home-layout-reset"
                disabled={busy}
                onClick={() => {
                  if (
                    window.confirm(
                      "Reset the home layout to its default order and visibility? This syncs to your other Nuvio devices.",
                    )
                  )
                    void update({ action: "reset" });
                }}
              >
                Reset to Default
              </button>
            </div>

            {items.length === 0 ? (
              <div className="collection-settings-empty">
                No catalogs or collections yet. Add an addon to get started.
              </div>
            ) : (
              <ul className="settings-list home-layout-list">
                {items.map((item, index) => (
                  <li
                    key={item.key}
                    className={[
                      "home-layout-row",
                      item.enabled ? "" : "hidden-row",
                      dragKey === item.key ? "dragging" : "",
                      dropIndex === index ? "drop-target" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    draggable={!busy && !item.pinnedToTop}
                    onDragStart={() => setDragKey(item.key)}
                    onDragEnd={() => {
                      setDragKey(null);
                      setDropIndex(null);
                    }}
                    onDragOver={(event) => {
                      if (!dragKey) return;
                      event.preventDefault();
                      setDropIndex(index);
                    }}
                    onDrop={(event) => {
                      event.preventDefault();
                      commitDrop(index);
                    }}
                  >
                    <span
                      className="home-layout-handle"
                      aria-hidden="true"
                      title={
                        item.pinnedToTop
                          ? "Pinned collections stay at the top"
                          : "Drag to reorder"
                      }
                    >
                      <Icon name="drag" size={17} />
                    </span>
                    <div className="home-layout-row-copy">
                      {editingKey === item.key ? (
                        <TitleEditor
                          value={titleDraft}
                          placeholder={item.defaultTitle}
                          onChange={setTitleDraft}
                          onCancel={() => setEditingKey(null)}
                          onCommit={() => {
                            setEditingKey(null);
                            if (titleDraft.trim() !== item.customTitle.trim())
                              void update({
                                action: "setCustomTitle",
                                key: item.key,
                                title: titleDraft,
                              });
                          }}
                        />
                      ) : (
                        <button
                          className="home-layout-title"
                          title="Rename this row"
                          disabled={busy}
                          onClick={() => {
                            setEditingKey(item.key);
                            setTitleDraft(item.customTitle);
                          }}
                        >
                          <strong>{item.displayTitle}</strong>
                          <Icon name="edit" size={14} />
                        </button>
                      )}
                      <span>{item.subtitle}</span>
                      <span className="home-layout-status">
                        {item.enabled ? "Visible" : "Hidden"}
                        {item.isCollection
                          ? item.pinnedToTop
                            ? " • Pinned to top"
                            : ""
                          : item.heroSourceEnabled
                            ? " • Hero source"
                            : " • Not in hero"}
                      </span>
                    </div>
                    <label className="switch">
                      <input
                        type="checkbox"
                        aria-label={`Show ${item.displayTitle} on Home`}
                        checked={item.enabled}
                        disabled={busy}
                        onChange={(event) =>
                          update({
                            action: "setEnabled",
                            key: item.key,
                            enabled: event.target.checked,
                          })
                        }
                      />
                      <i />
                    </label>
                    <div className="reorder-buttons">
                      <button
                        aria-label={`Move ${item.displayTitle} up`}
                        disabled={busy || index === 0 || item.pinnedToTop}
                        onClick={() =>
                          update({ action: "move", from: index, to: index - 1 })
                        }
                      >
                        <Icon name="up" size={17} />
                      </button>
                      <button
                        aria-label={`Move ${item.displayTitle} down`}
                        disabled={
                          busy || index === items.length - 1 || item.pinnedToTop
                        }
                        onClick={() =>
                          update({ action: "move", from: index, to: index + 1 })
                        }
                      >
                        <Icon name="down" size={17} />
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
      </div>
    </section>
  );
}

function ToggleRow({
  title,
  detail,
  checked,
  busy,
  onChange,
}: {
  title: string;
  detail: string;
  checked: boolean;
  busy: boolean;
  onChange(value: boolean): void;
}) {
  return (
    <div className="setting-row">
      <div>
        <strong>{title}</strong>
        <span>{detail}</span>
      </div>
      <label className="switch">
        <input
          type="checkbox"
          aria-label={title}
          checked={checked}
          disabled={busy}
          onChange={(event) => onChange(event.target.checked)}
        />
        <i />
      </label>
    </div>
  );
}

function TitleEditor({
  value,
  placeholder,
  onChange,
  onCommit,
  onCancel,
}: {
  value: string;
  placeholder: string;
  onChange(value: string): void;
  onCommit(): void;
  onCancel(): void;
}) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  return (
    <input
      ref={input}
      className="home-layout-title-input"
      value={value}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
      onBlur={onCommit}
      onKeyDown={(event) => {
        if (event.key === "Enter") onCommit();
        if (event.key === "Escape") onCancel();
      }}
    />
  );
}

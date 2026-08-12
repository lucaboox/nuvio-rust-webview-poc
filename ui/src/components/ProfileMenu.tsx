import { FormEvent, useMemo, useState } from "react";
import type { AvatarCatalogItem, NuvioProfile } from "../bridge/types";
import { Icon } from "./Icon";

export function ProfileMenu({ profiles, activeIndex, avatars, busy, canCreate, onSelect, onCreate, onSignOut }: { profiles: NuvioProfile[]; activeIndex: number; avatars: AvatarCatalogItem[]; busy: boolean; canCreate: boolean; onSelect(index: number): void; onCreate(input: { name: string; avatarId?: string; avatarUrl?: string; usesPrimaryAddons: boolean }): Promise<void>; onSignOut(): void }) {
  const [open, setOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const active = profiles.find((profile) => profile.profileIndex === activeIndex) ?? profiles[0];
  const avatarMap = useMemo(() => new Map(avatars.map((avatar) => [avatar.id, avatar])), [avatars]);

  function imageFor(profile?: NuvioProfile) { return profile?.avatarUrl || (profile?.avatarId ? avatarMap.get(profile.avatarId)?.imageUrl : undefined); }
  return <div className="profile-menu-wrap">
    <button className="profile-trigger" aria-label="Profiles" title={active?.name || "Profiles"} onClick={() => setOpen(!open)} disabled={busy}><ProfileAvatar profile={active} image={imageFor(active)} /></button>
    {open && <><button className="profile-menu-dismiss" aria-label="Close profiles" onClick={() => setOpen(false)} /><section className="profile-popover"><div className="profile-popover-head"><span>WHO'S WATCHING?</span><strong>{active?.name}</strong></div><div className="profile-icon-grid">{profiles.map((profile) => <button key={profile.profileIndex} className={profile.profileIndex === activeIndex ? "active" : ""} disabled={busy} onClick={() => { onSelect(profile.profileIndex); setOpen(false); }}><ProfileAvatar profile={profile} image={imageFor(profile)} /><span>{profile.name}</span></button>)}</div>{canCreate && profiles.length < 6 && <button className="profile-add" onClick={() => setCreating(true)}><i><Icon name="plus" size={19} /></i><span>Add profile</span></button>}<button className="profile-signout" onClick={onSignOut}><Icon name="logout" size={18} /><span>Sign out</span></button></section></>}
    {creating && <CreateProfileDialog avatars={avatars} onClose={() => setCreating(false)} onCreate={async (input) => { await onCreate(input); setCreating(false); setOpen(false); }} />}
  </div>;
}

function ProfileAvatar({ profile, image }: { profile?: NuvioProfile; image?: string }) {
  return <span className="profile-avatar" style={{ background: profile?.avatarColorHex || "#6b7280" }}>{image ? <img src={image} alt="" /> : <b>{(profile?.name || "N").slice(0, 1).toUpperCase()}</b>}</span>;
}

function CreateProfileDialog({ avatars, onClose, onCreate }: { avatars: AvatarCatalogItem[]; onClose(): void; onCreate(input: { name: string; avatarId?: string; avatarUrl?: string; usesPrimaryAddons: boolean }): Promise<void> }) {
  const [name, setName] = useState("");
  const [selected, setSelected] = useState(avatars[0]?.id || "");
  const [customUrl, setCustomUrl] = useState("");
  const [inherit, setInherit] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function submit(event: FormEvent) {
    event.preventDefault(); if (!name.trim()) return;
    setBusy(true); setError(null);
    try { await onCreate({ name: name.trim(), avatarId: customUrl.trim() ? undefined : selected || undefined, avatarUrl: customUrl.trim() || undefined, usesPrimaryAddons: inherit }); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "Could not create profile"); setBusy(false); }
  }
  return <div className="profile-dialog-scrim" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}><form className="profile-dialog" onSubmit={submit}><button type="button" className="profile-dialog-close icon-close-button" aria-label="Close" onClick={onClose}><Icon name="close" size={22} /></button><span>NEW PROFILE</span><h2>Add a profile</h2><p>Profiles keep libraries, progress, settings and addon preferences separate.</p><label className="profile-name-input"><span>Name</span><input autoFocus maxLength={40} value={name} onChange={(event) => setName(event.target.value)} placeholder="Profile name" /></label>{avatars.length > 0 && <div className="avatar-picker"><span>Choose an avatar</span><div>{avatars.map((avatar) => <button type="button" key={avatar.id} title={avatar.displayName} className={!customUrl && selected === avatar.id ? "selected" : ""} onClick={() => { setSelected(avatar.id); setCustomUrl(""); }} style={{ background: avatar.bgColor }}><img src={avatar.imageUrl} alt={avatar.displayName} /></button>)}</div></div>}<label className="profile-name-input"><span>Custom image URL <small>optional</small></span><input value={customUrl} onChange={(event) => setCustomUrl(event.target.value)} placeholder="https://…" /></label><label className="profile-inherit"><span><strong>Use primary profile addons</strong><small>Share the primary profile's Stremio addons</small></span><input type="checkbox" checked={inherit} onChange={(event) => setInherit(event.target.checked)} /></label>{error && <div className="inline-error">{error}</div>}<button className="profile-create-submit" disabled={busy || !name.trim()}>{busy ? "Creating…" : "Create profile"}</button></form></div>;
}

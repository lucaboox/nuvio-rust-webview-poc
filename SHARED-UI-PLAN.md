# One UI, two shells

Plan for `feat/shared-web-ui`: the Rust client renders the Nuvio Web UI, and
keeps the things only a desktop can do.

Nothing has been ported yet. This is the design, written while the two
codebases were fresh, so the work does not start by rediscovering it.

Steps 1 and 2 are done. The web client has the capability layer in
`src/platform/`, and this shell renders that UI: signed in, catalogs loading,
nothing about the web build changed.

Two things the plan did not anticipate, both now settled:

- **The network is a capability too.** The data layer was the whole of the
  difference, not just auth and storage — twelve `fetch` calls here against
  eighty `invoke` calls there, and a CSP allowing `connect-src ipc:` alone.
  `platform.request` is answered by `fetch` in a browser and by Rust here.
- **Auth was decided in favour of the shell.** A browser hides the token in a
  Worker; this keeps it outside the webview entirely, which is the same promise
  kept somewhere stronger. `auth.request` names a path, the shell signs it, and
  no token crosses back.

The substitution is a build-time alias — `NUVIO_PLATFORM_MODULE` points at
`shell-ui/platform.ts` — so the submodule stays pristine and exactly one file
differs per shell, as intended.

A warning worth keeping, since it cost two rounds of blaming the wrong thing:
the service worker really must not be ported. It precaches the app and waits to
be prompted before updating, so once registered in the webview it went on
serving its own copy and every later change appeared not to apply. It is
disabled for shell builds now, but an already-registered worker survives that
and has to be cleared from the webview profile by hand.

## Why this is possible at all

Both UIs are React and TypeScript built by Vite. The Rust client is a webview
around `ui/`, so replacing `ui/` with the web client's `src/` is a swap, not a
rewrite. There is no Compose, no native view layer, nothing to translate.

## The one decision everything else follows from

**The shared UI must never test for "am I in Tauri".** Scatter that check and
every future feature needs a desktop branch and a web branch, which is the
divergence this is meant to end.

Instead the UI asks for a capability and the shell supplies it:

```ts
// ui/src/platform/index.ts — the only file that differs per shell
export type Platform = {
  downloads?: DownloadsApi;   // desktop only; undefined on the web
  debrid?: DebridApi;         // desktop only — the browser cannot reach Torbox
  externalPlayer: ExternalPlayerApi;
  storage: StorageApi;
};
```

The UI renders a Downloads page when `platform.downloads` exists and does not
when it does not. Same source, no branching on shell identity, and a feature
added to the web client works in the desktop one the day it lands.

This is the pattern the web client already uses for optional things —
`onCreate` on `ProfileGate`, `onMenu` on the poster cards — where absence
removes the affordance rather than a flag disabling it.

## What is genuinely desktop-only

Everything else is shared. This list is short on purpose; if it grows, the
capability boundary is in the wrong place.

- **Downloads** — `DownloadsPage`, `DownloadSettingsSection`, and the queue in
  `shell/src/downloads.rs`. No browser equivalent.
- **Debrid / Connected Services** — Torbox sends no cross-origin headers, so
  the browser cannot reach it at all. The web client's Integrations page says
  so; the desktop one would show real controls.
- **Local backend config** — `shell/src/settings.rs` holds machine-level state
  the web client keeps in `localStorage`.
- **Native player** — anything that is not the browser's own decoder.

## What must not be ported

Web-only workarounds, which the desktop shell does not need and which would be
inexplicable in a shared file:

- `scroll-padding` and `overscroll-behavior` fixes (Chromium scroll-chain
  behaviour in a browser).
- The IMDb ratings Worker (exists solely because a page cannot call the
  ratings host — a desktop shell can call it directly).
- The return relay and Shortcut flow (an installed iOS web app cannot be
  reached by URL).
- `svh` sizing, address-bar compensation, PWA service worker.

Keep them behind the capability layer or leave them in the web build.

## Order of work

Each step should end with something that runs.

1. **Capability layer first**, in the *web* client, with everything optional
   absent. Nothing changes for the web; it just becomes portable.
2. **Point the Rust shell at the web UI** and get it to boot. Expect auth and
   storage to need the shell's versions.
3. **Reattach downloads** through `platform.downloads`. The existing
   `DownloadsPage` moves across mostly intact.
4. **Delete `ui/`'s duplicates** only once their replacements work.
5. **Settings last.** `settingsRegistry.ts` is data-driven, so the registry
   becomes the shared one plus desktop entries the capability layer adds.

## Two things to check early, before committing to the plan

- **Auth.** The web client keeps access tokens in a Worker
  (`src/workers/authWorker.ts`) so they never touch the main thread. The Rust
  shell has its own auth in `shell/src/auth.rs`. Decide which owns the session
  before porting anything that reads it, or both will and neither will work.
- **Storage.** The web client uses IndexedDB via `src/lib/idb.ts`. The shell
  writes files. `platform.storage` has to cover both, and the shapes differ
  more than they look.

## What this buys

One place to fix a bug. A feature written once appears in both. The Rust
client stops being a thing that falls behind every time the web client
improves — which is the actual problem being solved here, not the UI itself.

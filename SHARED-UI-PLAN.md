# One UI, two shells

Plan for `feat/shared-web-ui`: the Rust client renders the Nuvio Web UI, and
keeps the things only a desktop can do.

## Checkout and build discipline

There are **two working checkouts of the same `nuvio-web` repository**:

- `../Web-Version` is the standalone checkout.
- `shared-ui` is the desktop repository's Git submodule.

They can point at the same branch and commit while still being separate working
directories. A change made in one is not visible in the other. Edit only one
checkout, commit and push it, then pull or update the other checkout. Never make
the same change independently in both directories.

Shared UI work belongs on `feat/shared-web-ui` and must be committed and pushed.
Do not leave changes uncommitted in the submodule: `git submodule update` can
silently discard them while moving the submodule back to the commit recorded by
the desktop repository.

The desktop application embeds the shared UI at compile time. After changing
the UI, rebuild the shared bundle and relink the Rust binary before testing it.
Close the running desktop application first because Cargo cannot replace a
running executable on Windows; otherwise it is easy to launch and test the old
binary while believing the new UI was included.

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
4. **Delete `ui/`'s duplicates** only once their replacements work. The word in
   this step is *duplicates*, and it has to be honoured module by module rather
   than by removing the folder — a wholesale delete would have taken the Debrid
   filtering rules with it, along with the tests behind them.

   That audit is now done, module by module against `Web-Version/src/lib`.
   Matching by name was never going to answer it: most of the counterparts were
   renamed on the way across, so each one had to be read.

   **Covered, under a different name** — `continueWatching`→`progress`,
   `seriesProgress`→`seriesPlayback`, `bingeGroupCache`→`bingeCache`,
   `streamAutoplay`→`nextEpisode`, `integrationSettings`→`providerCredentials`,
   `posterSize`+`clientSettings`→`webSettings`, `catalogLabels`→`mediaTypeLabel`
   and the row-naming inline in `addons.ts`, `debridStreams` ported outright.

   **Covered by a different mechanism** — `libraryCache` and `watchedOverrides`
   are caches and optimistic-update bookkeeping that the shared UI does against
   React state directly (`library.some(...)`, and the rollback in `App.tsx`), so
   there is nothing to port, only something not to reintroduce.
   `settingsRegistry` is 1114 lines of declarative setting definitions against a
   hand-written Settings page; the same settings, a different architecture.

   **`home.ts` is dead** — demo fixtures, no importers even inside `ui/`.

   **Genuinely unported, and the whole of what still blocks this step:**

   - `recentSearches` (49 lines) — search history. The shared UI's search has
     none.
   - `streamLinkCache` (175 lines) — reuses a resolved debrid link for 24h
     instead of resolving it again, and knows which URLs carry expiring
     credentials so it does not serve a dead one. Worth porting on its own
     merits: re-resolving costs a Torbox round trip every time.

   Port those two and `ui/` can go in one commit.
5. **The player, in the window.** The risk this step was flagged for turned out
   not to exist: the shell already composites libmpv into its own window, and
   the mechanism is four lines rather than a windowing project. What follows is
   the whole of it, read out of `ui/`'s `PlayerPage` and `styles/app.css`
   before that tree is deleted, because it is not obvious from either the Rust
   or the bridge.

   - **mpv renders behind the webview, always.** It is revealed by making the
     page transparent, not by moving anything: `html.player-active,
     body.player-active, body.player-active #root { background: transparent
     !important }`. Add the class to play, remove it to stop. That is the
     entire compositing story.
   - **`player.prepare` starts playback into that surface.** It does not open a
     window. Its parameters are `mediaId`, `url`, `externalUrl`,
     `requestHeaders`, `startPositionMs` and a `progress` object carrying
     contentId, contentType, videoId and season — the last is how the shell
     attributes watch progress, and omitting it loses that silently.
   - **State is polled, not pushed.** `PlayerPage` reads `player.state`; it
     never subscribes. `player.stateChanged` exists but nothing consumes it, so
     polling is the proven path and a subscription is an improvement, not a
     prerequisite.
   - **Controls are already there**: togglePause, seek, seekRelative, setVolume,
     toggleMute, cycleAudio, cycleSubtitle, setSpeed, setAudioTrack,
     setSubtitleTrack, stop, plus thumbnail and skipSegments.

   So the work is to widen `PlayerApi` past the handoff to cover those, and to
   give the shared `Player.tsx` a native mode: same chrome, transport calls
   going over the bridge instead of to an `HTMLVideoElement`, and the
   `player-active` class on while it is up. No architectural unknown remains.

   **The mechanism, read out of the Rust rather than guessed at.** mpv renders
   into the *same* HWND as WebView2 — `native.rs` says so outright: "a separate
   sibling video HWND cannot show through a windowed WebView2 surface". The
   handle is supplied once at startup by `configure_window` from `main.rs`, and
   `prepare` refuses to run without it. So there is no surface to attach and no
   ordering to get right: the video is always behind the page, and the only
   question is whether the page paints over it.

   **Which is why the second attempt showed nothing.** Making `html`, `body`
   and `#root` transparent is what the old UI needed, because its layout put no
   background anywhere else. The shared UI does — `body, #root` at line 18, and
   more on the layout containers inside it. Chasing those selector by selector
   is how this fails a third time.

   The robust rule is to stop rendering the app instead of trying to see
   through it: with the overlay as a direct child of the root, everything else
   under it gets `display: none` while the class is on. Nothing to hunt, and it
   cannot be defeated by a background added later. That requires the overlay to
   actually be a root-level child, which in `App.tsx` it currently is not.

   Stremio arrived at this same architecture — `stremio-shell-ng` is Rust,
   WebView2 and mpv rendering direct to the window, replacing their Qt shell
   for 2-5x the efficiency. Worth reading if the layering ever needs changing,
   though not for this: the layering here already works.

   Two mistakes already made here, both worth not repeating. `prepare` was
   wired up as though it opened its own window, which sent every stream to a
   surface nothing rendered and made clicking a source do nothing at all. And
   `void`-ing the promise meant the failure was silent — if a capability call
   can reject, something has to say so.

6. **Settings last.** `settingsRegistry.ts` is data-driven, so the registry
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

# Nuvio Desktop Alpha

An experimental Windows desktop client built with Rust, Tauri, React, TypeScript, WebView2, and direct libmpv playback. Read [ARCHITECTURE.md](./ARCHITECTURE.md) for the original repository audit, reuse boundaries, IPC design, player adaptation, and staged migration plan.

The install/update release process is documented in [RELEASING.md](./RELEASING.md). The current application version is **0.1 Alpha 1** (`0.1.0-alpha.1`).

## What works

- Tauri owns the native window and hosts React through the installed Microsoft WebView2 runtime.
- UI assets are embedded into the release binary.
- React calls the existing typed Rust command bridge through Tauri IPC.
- Rust returns correlated responses and unsolicited events to JavaScript.
- The UI provides a responsive Nuvio-style navigation and real addon-backed home shell.
- Email/password sign-in and sign-up use Nuvio's real Supabase Auth endpoint.
- Profiles and installed addon rows are loaded read-only from the authenticated account.
- Guest mode, profile switching, addon refresh, and sign-out are wired end to end.
- Enabled Stremio-compatible addon manifests and home/search catalogs load through Rust with partial-failure handling.
- Poster rows open rich addon metadata with cast imagery, trailers, season tabs, episode selection, and a separate concurrent source picker.
- Home shelves support mouse/touch dragging, catalog pages, and addon pagination.
- Stremio addons can be installed, enabled, reordered, configured, and removed through Nuvio's synced addon RPC.
- Core playback, subtitle, stream-badge, and notification settings read and update Nuvio's versioned desktop profile blob while preserving unknown fields.
- The gray desktop theme includes a synced AMOLED-black option.
- Library items use Nuvio's existing profile-scoped sync RPCs and can be added or removed from Details.
- Direct HTTP streams render through libmpv inside the main window; saved progress selects the resume episode, seeks to the saved position, and syncs playback progress.
- Signed application updates can be checked and installed from **Settings > Updates**.

This remains an incremental POC: it supports Stremio-compatible addon HTTP resources but does not execute Nuvio QuickJS plugins, and torrent-only sources still need a resolver/debrid layer before libmpv can open them. The Supabase refresh token is stored in Windows Credential Manager; access tokens remain memory-only and sign-out removes the saved credential.

## Prerequisites

- Windows 10/11 with the WebView2 Runtime (normally installed with Edge)
- Rust MSVC toolchain
- Node.js 20 or newer

## Build and run

From this directory:

```powershell
npm.cmd install
npm.cmd run check:ui
npm.cmd run build:ui
cargo test --manifest-path shell/Cargo.toml
npm.cmd run tauri -- dev --config shell/tauri.conf.json
```

The UI must be built before a direct Cargo build because the shell embeds `ui/dist` at compile time. The Tauri development command handles this through `beforeBuildCommand`.

The local `.env.local` contains the same public Supabase client URL and anon key embedded in official Nuvio builds. It is ignored by Git. Use `.env.example` when configuring another checkout.

Sign in with an existing Nuvio account to test real profiles and the read-only Addons page, or use guest mode. Use **Test round trip** to exercise React → Rust → React communication. **Play prototype** exercises the player command and Rust → React event path; it reports that libmpv is intentionally not loaded.

## Development notes

- `npm.cmd run dev:ui` can preview the React layout in a browser, but native bridge actions will correctly report that the bridge is unavailable.
- IPC work is dispatched away from the WebView UI thread. Addon requests fan out concurrently while preserving stable display order.

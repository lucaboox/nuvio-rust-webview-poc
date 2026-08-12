# Nuvio Rust/WebView2 POC

An isolated Windows desktop architecture experiment. Read [ARCHITECTURE.md](./ARCHITECTURE.md) first for the repository audit, reuse boundaries, IPC design, player adaptation, and staged migration plan.

## What works

- Rust owns a native Tao window.
- Wry hosts the React UI using the installed Microsoft WebView2 runtime.
- UI assets are embedded into the Rust binary at compile time and served through the `nuvio://` application protocol.
- React calls typed native commands through `window.ipc.postMessage`.
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
- Direct HTTP streams open in a Rust-owned native Windows/libmpv window; saved progress selects the resume episode and seeks to the saved position, then syncs the final position when the player closes.

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
cargo run --manifest-path shell/Cargo.toml
```

The UI must be built before Cargo because the shell embeds `ui/dist` at compile time. `npm.cmd run dev` performs the UI build and launches the shell in one command.

The local `.env.local` contains the same public Supabase client URL and anon key embedded in official Nuvio builds. It is ignored by Git. Use `.env.example` when configuring another checkout.

Sign in with an existing Nuvio account to test real profiles and the read-only Addons page, or use guest mode. Use **Test round trip** to exercise React → Rust → React communication. **Play prototype** exercises the player command and Rust → React event path; it reports that libmpv is intentionally not loaded.

## Development notes

- `npm.cmd run dev:ui` can preview the React layout in a browser, but native bridge actions will correctly report that the bridge is unavailable.
- Debug Rust builds enable WebView devtools through Wry.
- IPC work is dispatched off the Tao UI thread and returned through its event-loop proxy. Addon requests fan out concurrently while preserving stable display order.

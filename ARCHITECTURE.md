# Nuvio Rust + WebView2 proof of concept

## Purpose and constraints

This project is an isolated migration experiment. It does not replace, edit, or share a build with the current Kotlin/Compose application. Its job is to validate four boundaries cheaply:

1. a Rust-owned native Windows window;
2. a React/TypeScript UI rendered by WebView2;
3. typed request/response and event communication between the UI and Rust;
4. a player service boundary that can later host direct `libmpv` playback.

The current app remains the behavioral reference until a migrated vertical slice reaches parity. The proof of concept now includes real Supabase email authentication, read-only profile/addon loading, and Stremio-compatible addon catalogs, search, metadata, and stream lookup. The player remains a stub and QuickJS plugins remain a separate compatibility slice.

## Repository audit

### Realistically reusable with minimal semantic change

The following Kotlin is platform-neutral enough to port function-for-function into Rust, or initially expose through compatibility fixtures and contract tests:

- Stremio addon protocol models and URL behavior:
  - `features/addons/AddonModels.kt`
  - `features/addons/AddonManifestParser.kt`
  - `features/addons/AddonTransportUrls.kt`
- Catalog, search, and discover rules:
  - `features/catalog/*`
  - the request construction, pagination, merging, and catalog-selection portions of `features/search/SearchRepository.kt`
  - `features/home/HomeCatalogDefinitions.kt`, `HomeCatalogParser.kt`, and release filtering rules
- Metadata and series behavior:
  - `features/details/MetaDetailsParser.kt`
  - `SeriesSeasonSupport.kt`, `SeriesPlaybackResolver.kt`, and small formatting/selection policies
- Stream behavior:
  - `features/streams/StreamParser.kt`, `StreamModels.kt`, `PlaybackUrlCredentials.kt`
  - autoplay, badge, binge-group, and link-cache policies
  - addon stream fan-out and ordering semantics from `StreamsRepository.kt`
- Watch state and library domain rules:
  - watch-progress identity, rules, projections, and source coordination
  - watched/library/collection domain models, reconciliation rules, and serialization shapes
- Integration behavior for Trakt, Simkl, TMDB, MDBList, debrid providers, Supabase sync, and TorrServer. Their HTTP contracts and policies can be retained even though the Ktor/coroutine implementations need Rust equivalents.
- Existing tests are valuable as executable specifications. JSON fixtures and expected values should be moved into language-neutral contract tests before each port.
- Assets under `composeResources` can be copied or transformed into web assets. The POC reuses the Nuvio wordmark and visual concepts while leaving the originals in place.

"Reuse" here mostly means preserving behavior and data contracts, not compiling Kotlin into the new process. Keeping a JVM sidecar would retain code literally, but it would undermine the startup/memory goal and create a second IPC boundary. A direct, test-guided port is cleaner.

### Must be rewritten or adapted

- Every Compose screen/component, navigation host, animation, painter, modifier, and Compose resource lookup must become React components, CSS, router/state code, and web localization/assets. About 215 of the 481 `commonMain` Kotlin files import Compose or Compose resources, so source-level UI reuse is not realistic.
- Repository singletons built around `CoroutineScope`, `StateFlow`, Compose resources, and global mutable state need Rust services with explicit ownership. The policies inside them are reusable; their lifecycle and state plumbing are not.
- Ktor networking and `kotlinx.serialization` DTO decoding become a Rust HTTP client plus Serde models. Preserve tolerant decoding (`ignoreUnknownKeys` behavior), URL rules, concurrency, cancellation, ordering, and pagination.
- `expect`/`actual` storage implementations and Java `.properties` stores become a versioned Rust persistence layer. During migration, Rust should import existing `%APPDATA%/Nuvio/*.properties` payloads once and write new versioned JSON or SQLite data separately. Do not let both apps write the same files concurrently.
- Compose Desktop/AWT window management, Swing dispatch, deep-link handling, and JNI bindings must be replaced by Rust/Win32 equivalents.
- QuickJS plugin runtime integration is a separate migration slice. Preserve plugin manifests, persisted configuration, host APIs, and test fixtures; replace the Kotlin bindings/runtime host deliberately rather than running plugin code in the privileged UI WebView.

## Recommended runtime architecture

```text
React + TypeScript (untrusted presentation)
  | typed JSON-RPC requests, responses, events
  v
Rust command router (validation, correlation IDs, error mapping)
  |-- catalog/addon/search/metadata services
  |-- settings/watch/library persistence
  |-- integration clients and sync coordinator
  |-- plugin runtime sandbox (later)
  `-- player service
        `-- Windows native player bridge -> libmpv child HWND
```

Rust is the source of truth for durable state, credentials, network access, addon execution, playback, and OS integration. React owns transient presentation state only. This prevents credentials and unrestricted filesystem/network capability from leaking into WebView JavaScript.

### Rust to WebView communication

Use Wry's WebView2 backend and its IPC handler. The wire protocol is JSON-RPC-like but intentionally small:

```json
{ "id": "1", "method": "app.bootstrap", "params": {} }
{ "id": "1", "ok": true, "result": { "platform": "windows" } }
{ "event": "player.stateChanged", "payload": { "paused": false } }
```

- TypeScript owns a generated/checked client wrapper and correlates promises by `id`.
- Rust deserializes an envelope first, then validates method-specific parameters.
- Responses and events return through one initialization-script function, evaluated by Rust on the UI thread.
- Long operations return promptly and run on a worker/runtime; completion is posted back through the native event-loop proxy. Cancellation should be a first-class method once real searches are added.
- Commands are namespaced (`app.*`, `addons.*`, `catalog.*`, `metadata.*`, `settings.*`, `watch.*`, `player.*`) and versioned at the protocol level before compatibility matters.
- Only the bundled application origin/custom protocol may invoke privileged commands. Navigation to unknown origins and new-window requests should be rejected or opened in the system browser.
- Never expose a generic shell, arbitrary filesystem, arbitrary SQL, raw token, or unrestricted HTTP command to JavaScript.

The POC dispatches bridge requests to worker threads so addon networking does not block the Tao/WebView UI loop. Blocking Rust HTTP clients stay isolated on those workers; a production core can retain the service contracts while adding async cancellation and streaming.

## Preserving product behavior

### Addons, home, search, discover, metadata, and streams

Port the protocol bottom-up:

1. Serde models matching `AddonManifest`, resources, catalogs, extras, and behavior hints.
2. Exact manifest normalization and addon resource URL construction, with Kotlin test cases shared as fixtures.
3. Manifest persistence, enabled/order state, tolerant refresh, and profile scoping.
4. Catalog/meta/stream response parsers using the current data shapes.
5. Concurrent fan-out with stable addon ordering, partial results, cancellation, pagination, release filtering, stream parsing, badge rules, and autoplay selection.
6. Supabase/profile sync only after local behavior is equivalent.

Do not fetch addon URLs directly from React. Rust should own requests so headers, DNS/TLS policy, cancellation, caching, diagnostics, and future plugin/debrid resolution stay consistent and testable.

### Settings and watch progress

- Define explicit schema-versioned Rust records rather than mirroring the current many-file storage API forever.
- Add a read-only importer for current `.properties` files and their JSON payload values.
- Preserve profile scoping, watch identity keys, completion thresholds, resume behavior, enrichment caches, and provider reconciliation exactly through shared fixtures.
- Keep player progress events native-to-Rust. Persist on meaningful intervals and lifecycle transitions; emit throttled display updates to React.

### Playback and existing Windows native work

The current `player_bridge.cpp` is highly reusable. It already:

- loads `libmpv-2.dll` dynamically and supports `NUVIO_LIBMPV_PATH`;
- embeds mpv with `wid` into a child HWND;
- configures GPU/hardware decoding, headers, cache, tone mapping, scaling, and RTX VSR;
- owns playback, seeking, volume, tracks, subtitles, subtitle styling, resize modes, and polling;
- creates a transparent WebView2 controls overlay and handles JS messages;
- handles focus, fullscreen chrome, display-sleep inhibition, native teardown, and thread/message-loop constraints.

Recommended adaptation:

1. Copy the native player into this experiment only when playback work begins; do not edit the production bridge in place.
2. Split player core/Win32/WebView controls from the JNI adapter.
3. Export a stable C ABI (`nuvio_player_create`, command functions, destroy, callback registration). Rust calls it through a small unsafe FFI module and exposes a safe `PlayerService` actor.
4. Replace `JavaVM`/`jobject` callbacks with a C function pointer plus opaque context, or post player events to a Rust-owned message HWND.
5. Initially keep the proven native controls overlay. Later decide whether to fold controls into the main React app; doing so requires careful HWND/WebView z-order and fullscreen behavior and is not needed to prove the shell architecture.
6. Reuse the existing bundled `libmpv-2.dll` and runtime dependency packaging, subject to license/release validation. Do not load the DLL in this shell POC yet.

Using Wry for the app shell does not conflict with the player's existing WebView2 controls. They can use separate controllers and a shared or separate WebView2 environment/user-data directory. Keep player lifetime independent of the main UI so navigation or React crashes cannot leak an mpv handle.

## Proposed folder structure

```text
experiments/rust-webview-poc/
  ARCHITECTURE.md          # this design and migration plan
  README.md                # build/run instructions and POC limits
  package.json             # React/Vite scripts only
  vite.config.ts
  tsconfig*.json
  ui/
    index.html
    src/
      app/                 # navigation and shell composition
      bridge/              # typed Rust IPC client and wire types
      components/          # reusable Nuvio-style web UI
      data/                # temporary POC presentation fixtures
      styles/
  shell/
    Cargo.toml
    src/
      main.rs              # Tao window + Wry WebView2 lifecycle
      assets.rs            # embeds/serves the built UI through an app protocol
      ipc.rs               # validated command envelopes/router
      app_state.rs         # Rust-owned application state
      player/
        mod.rs             # safe service API; stub in this POC
        ffi.rs             # reserved for the C ABI adapter
```

The production migration should eventually become a top-level workspace after the experiment is accepted. Keeping it under `experiments/` now avoids coupling Gradle, Cargo, and npm release flows prematurely.

## Migration stages and gates

1. **Shell POC (implemented):** native window, WebView2 React shell, bidirectional IPC, player-service seam, memory-only email/guest authentication, real profile loading, and read-only synced addon rows. Gate: Rust tests, TypeScript build, Rust build, backend configuration check, manual launch.
2. **Protocol core (initial implementation):** addon manifest/URL/catalog/meta/stream models, URL contract tests, stable concurrent fan-out, and partial failures. Gate remaining: expand shared Kotlin/Rust fixtures and pagination cases.
3. **Read-only vertical slice (implemented baseline):** installed-addon home/search/details/episode/stream listing with no writes to legacy stores. Discover filters, pagination, ordering parity, and QuickJS plugin results remain before this gate is complete.
4. **New persistence and settings:** versioned store, one-time legacy import, profile scope, settings UI, watch progress. Gate: migration tests and recovery/rollback story.
5. **Direct playback:** C ABI extraction from the existing Windows player, Rust actor, stream headers, tracks/subtitles/progress/fullscreen. Gate: playback smoke matrix and leak/teardown tests.
6. **Integrations:** Trakt/Simkl/TMDB/MDBList, debrid, Supabase sync, TorrServer, deep links, downloads, updater, diagnostics. Gate each independently.
7. **Plugin runtime:** sandboxed QuickJS/WASM host with the existing manifest/host API contracts. Gate: current plugin tests and permission boundaries.
8. **Parity and cutover:** accessibility, keyboard/remote behavior, localization, packaging, signing, updater, telemetry/privacy, performance comparison, and opt-in beta. Retire Compose Desktop only after measured parity and a rollback window.

## Explicit non-goals for this POC

- No credential migration, account writes, torrent engine, QuickJS plugin execution, or libmpv loading. Stremio-compatible addon manifest/catalog/meta/stream traffic is now real and owned by Rust.
- No changes to Gradle, Compose sources, current player native sources, or current storage.
- No claim that the mock home screen is a pixel-perfect port.

/**
 * The desktop shell, as the shared UI meets it.
 *
 * This is the file the plan says differs per shell, and it is the only one: the
 * web client's `src/platform/index.ts` is aliased to this at build time, so
 * every module above it goes on importing `platform` and never learns which
 * client it got.
 *
 * It lives here rather than in the submodule on purpose. `shared-ui` is a
 * pinned checkout of another repository, and a shell that edited it would
 * dirty the submodule and lose the change on the next update.
 */

import { open } from "@tauri-apps/plugin-dialog";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { invoke } from "./bridge.ts";
import { copyStreamUrl } from "../shared-ui/src/lib/externalPlayer.ts";
import { deleteValue, getValue, setValue } from "../shared-ui/src/lib/idb.ts";
import type {
  BackendConfig,
  Session,
} from "../shared-ui/src/types.ts";
import type {
  DownloadItem,
  DownloadRequest,
  DownloadsSnapshot,
  Platform,
  PlayerSource,
  PlayerState,
  RequestOptions,
  RequestResponse,
} from "../shared-ui/src/platform/types.ts";

export type * from "../shared-ui/src/platform/types.ts";

/**
 * HTTP by way of Rust, because the webview cannot do it itself.
 *
 * The CSP here allows `connect-src ipc:` and nothing else, which is what keeps
 * an installed addon from having a browser context inside the desktop app. The
 * shell dials instead, and enforces the timeout, the size cap and the scheme
 * rules on its side of the hop.
 */
const request = (url: string, options: RequestOptions = {}) =>
  invoke<RequestResponse>("http.request", {
    url,
    method: options.method,
    headers: options.headers,
    body: options.body,
    timeoutMs: options.timeoutMs,
    maxBytes: options.maxBytes,
  });

/** Shapes the queue's own snapshot into what the shared contract promises. */
const downloads = {
  list: () => invoke<DownloadsSnapshot>("downloads.list"),
  enqueue: (item: DownloadRequest) =>
    invoke<{ item: DownloadItem }>("downloads.enqueue", { request: item }).then(
      () => undefined,
    ),
  cancel: (id: string) => invoke<unknown>("downloads.cancel", { id }).then(() => undefined),
  retry: (id: string) => invoke<unknown>("downloads.retry", { id }).then(() => undefined),
  remove: (id: string) => invoke<unknown>("downloads.remove", { id }).then(() => undefined),
  artwork: (id: string) =>
    invoke<{ image?: string }>("downloads.artwork", { id }).then(
      (result) => result.image ?? null,
    ),
  openFolder: () => invoke<unknown>("downloads.openFolder").then(() => undefined),
  // The native picker, which is the whole reason this is a capability: the
  // shared page cannot import a Tauri plugin, and only a shell can offer a
  // directory chooser at all.
  chooseFolder: (current?: string) =>
    open({ directory: true, multiple: false, defaultPath: current }).then(
      (value) => (typeof value === "string" ? value : null),
    ),
  moveStorage: (path: string) =>
    invoke<unknown>("downloads.moveStorage", { path }).then(() => undefined),
};

/**
 * The session lives in the shell, not in the page.
 *
 * The browser keeps it in a Worker so the page cannot read the token. Here it
 * never enters the webview at all — the same promise kept somewhere stronger,
 * because there is no JavaScript context that could reach it even in
 * principle. What crosses the bridge is a path and a body; the shell signs.
 */
/**
 * The shell's account payload, as the shared UI expects a session.
 *
 * They are not the same shape and never were: the shell answers with
 * `{ auth: { userId, email, backendUrl, ... }, profiles, ... }` while the UI
 * wants `{ user: { id, email }, backend }`. Passing one off as the other
 * happened to work for as long as nothing read `user` — Settings was the first
 * page to, and it took the whole app down with it rather than showing a
 * missing address.
 */
type ShellAccount = {
  auth?: {
    userId?: string;
    email?: string;
    backendUrl?: string;
    selfHosted?: boolean;
  };
};

function toSession(payload: ShellAccount): Session {
  const account = payload.auth ?? {};
  return {
    user: { id: account.userId ?? "", email: account.email },
    backend: {
      url: account.backendUrl ?? "",
      // The shell holds the publishable key and does not hand it back, which
      // is the point of it owning the session. Nothing in the UI reads this;
      // it is here because the type says a session has a backend.
      key: "",
      selfHosted: account.selfHosted ?? false,
    },
  };
}

const auth = {
  signIn: (backend: BackendConfig, email: string, password: string) =>
    invoke<unknown>("auth.configureBackend", {
      url: backend.url,
      key: backend.key,
      selfHosted: backend.selfHosted,
    })
      .then(() => invoke<ShellAccount>("auth.signIn", { email, password, sharedUi: true }))
      .then(toSession),
  // Restores rather than reports. `auth.state` answers with whatever session
  // is loaded, which on a fresh launch is none — the shell has to be asked to
  // rotate the stored credential first.
  restore: () => invoke<ShellAccount>("auth.restore", { sharedUi: true }).then(toSession),
  signOut: () => invoke<unknown>("auth.signOut").then(() => undefined),
  request: <T>(
    path: string,
    init: { method?: string; body?: string; headers?: Record<string, string> } = {},
  ) => invoke<T>("auth.request", { path, init }),
  // The shell's session outlives the page: a reload finds it still signed in,
  // and there is no worker here to crash out from under us. Nothing to
  // announce, so this registers a listener that is never called rather than
  // pretending the event cannot exist.
  onSessionLost: () => () => undefined,
};

/**
 * libmpv, by way of the shell.
 *
 * `player.prepare` is what the old UI called to start a file, and it already
 * refuses a `file:` URL that does not belong to a download — so offline
 * playback reaches the one player that can open it without widening anything.
 */
const player = {
  open: (source: PlayerSource) =>
    invoke<unknown>("player.prepare", {
      url: source.url,
      externalUrl: source.externalUrl,
      mediaId: source.mediaId,
      startPositionMs: source.startPositionMs ?? 0,
      requestHeaders: source.requestHeaders ?? {},
      deviceLanguages: source.deviceLanguages ?? [],
      contentLanguage: source.contentLanguage,
      preferences: source.preferences,
      // How the shell files what was watched. Without it playback works and
      // the history does not.
      progress: source.progress,
    }).then(() => undefined),
  state: () => invoke<PlayerState>("player.state"),
  skipSegments: (options: { contentId: string; videoId: string; season?: number; episode?: number; animeSkipEnabled: boolean; animeSkipClientId: string }) =>
    invoke<{ segments: Array<{ startMs: number; endMs: number; type: string; provider: string }> }>("player.skipSegments", options).then((result) => result.segments),
  togglePause: () => invoke<unknown>("player.togglePause").then(() => undefined),
  seek: (positionMs: number) =>
    invoke<unknown>("player.seek", { positionMs }).then(() => undefined),
  seekRelative: (offsetMs: number) =>
    invoke<unknown>("player.seekRelative", { offsetMs }).then(() => undefined),
  setVolume: (volume: number) =>
    invoke<unknown>("player.setVolume", { volume }).then(() => undefined),
  toggleMute: () => invoke<unknown>("player.toggleMute").then(() => undefined),
  setMuted: (muted: boolean) =>
    invoke<unknown>("player.setMuted", { muted }).then(() => undefined),
  setSpeed: (speed: number) =>
    invoke<unknown>("player.setSpeed", { speed }).then(() => undefined),
  setResizeMode: (mode: string) =>
    invoke<unknown>("player.setResizeMode", { mode }).then(() => undefined),
  setAudioTrack: (id: number) =>
    invoke<unknown>("player.setAudioTrack", { id }).then(() => undefined),
  setSubtitleTrack: (id: number) =>
    invoke<unknown>("player.setSubtitleTrack", { id }).then(() => undefined),
  setFullscreen: (fullscreen: boolean) =>
    invoke<unknown>("window.setFullscreen", { enabled: fullscreen }).then(() => undefined),
  stop: () => invoke<unknown>("player.stop").then(() => undefined),
  /*
   * RTX Video Super Resolution, which the Rust side already applies from the
   * shared settings blob — this is only what puts the switch on the screen.
   *
   * Declared here rather than discovered by the UI: the settings page must not
   * ask what it is running on. The native player is `cfg(windows)` and the
   * pipeline is d3d11, so nowhere else can offer this, and the browser build
   * simply omits the field.
   *
   * `appliesToCurrentPlayback` is false because `gpu-api` and `d3d11-adapter`
   * are read once, before mpv initialises. The filter itself could be added
   * mid-playback, but only in a session that already started on d3d11 with the
   * NVIDIA adapter — so a toggle would work or not depending on how playback
   * began, which is worse than saying plainly that it takes effect next time.
   */
  videoEnhancement: {
    kind: "rtx-super-resolution" as const,
    appliesToCurrentPlayback: false,
  },
};

/**
 * The episode-ratings service, called directly rather than through the Worker.
 *
 * The Worker is there because a browser cannot reach that service at all, and
 * it enforces an origin allowlist to stop anyone else spending its budget. This
 * shell has no origin to offer — every request it sent was refused with a 403,
 * which is why episode scores were blank here — and it does not need the proxy
 * in the first place, because Rust does the asking.
 *
 * Compiled in, so an installation without the address configured simply falls
 * back to the Worker rather than losing the badges.
 */
const ratingsBase = (
  import.meta.env.VITE_NUVIO_IMDB_RATINGS_BASE_URL ?? ""
).trim().replace(/\/+$/, "");

/**
 * The real application update, which the browser's service worker cannot be.
 *
 * The signed feed, the pubkey and the release workflow were all still in place
 * — only the screen that drove them went, with the old `ui/` tree. This puts
 * the same three steps behind the capability so the shared Settings control
 * works here without knowing any of it: ask, download, restart.
 *
 * The `Update` handle from `check()` is held between the two calls because
 * `downloadAndInstall` belongs to it; asking twice would download twice.
 */
let pendingUpdate: Awaited<ReturnType<typeof check>> = null;

const updates = {
  currentVersion: () => getVersion(),
  check: async () => {
    pendingUpdate = await check();
    return pendingUpdate
      ? {
          version: pendingUpdate.version,
          notes: pendingUpdate.body ?? undefined,
          date: pendingUpdate.date ?? undefined,
        }
      : null;
  },
  install: async (onProgress?: (fraction: number) => void) => {
    const update = pendingUpdate ?? (await check());
    if (!update) return;
    pendingUpdate = update;
    let total = 0;
    let received = 0;
    await update.downloadAndInstall((event) => {
      // The feed does not always declare a length; without one there is no
      // fraction to report and the caller shows an indeterminate wait rather
      // than a bar that never moves.
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      else if (event.event === "Progress") {
        received += event.data.chunkLength;
        if (total > 0) onProgress?.(Math.min(1, received / total));
      } else if (event.event === "Finished") onProgress?.(1);
    });
  },
  relaunch: () => relaunch(),
};

export const platform: Platform = {
  auth,
  player,
  downloads,
  ratings: ratingsBase ? { seasonRatingsBase: ratingsBase } : undefined,
  /**
   * Reachable from here, unlike from a page.
   *
   * The keys themselves are the account's and sync to every client; what a
   * browser cannot do is use them, because Torbox sends no cross-origin
   * headers. This shell makes its requests from Rust, so it can.
   *
   * The rows are the ones Nuvio seeds — "debrid:torbox" and friends, each
   * keeping its key under api_key — so a key set here appears on the phone and
   * the TV without anything further.
   */
  debrid: {
    services: [
      {
        id: "torbox",
        label: "Torbox",
        credentialProvider: "debrid:torbox",
        credentialField: "api_key",
      },
      {
        id: "premiumize",
        label: "Premiumize",
        credentialProvider: "debrid:premiumize",
        credentialField: "api_key",
      },
      {
        id: "realdebrid",
        label: "Real-Debrid",
        credentialProvider: "debrid:realdebrid",
        credentialField: "api_key",
      },
    ],
  },
  updates,
  request,
  // The webview has IndexedDB like any other, so the browser's implementation
  // is reused rather than reimplemented over the bridge. What belongs in files
  // is machine-level configuration, which `settings.rs` already owns and which
  // does not come through this contract.
  storage: {
    get: getValue,
    set: setValue,
    remove: deleteValue,
  },
  // The desktop build has one playback path: the native libmpv surface above.
  // Do not inherit browser URL-scheme players here. Besides being confusing in
  // Settings, those schemes can hand a stream out of the app without the native
  // progress and track-selection behavior the desktop client promises.
  externalPlayer: {
    options: () => [],
    label: () => "Internal libmpv",
    isAvailable: (mode) => mode === "internal",
    launch: () => undefined,
    copyUrl: copyStreamUrl,
  },
};

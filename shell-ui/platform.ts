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

import { invoke } from "./bridge.ts";
import {
  copyStreamUrl,
  externalPlayerLabel,
  externalPlayerOptions,
  isExternalPlayerAvailable,
  launchExternalPlayer,
} from "../shared-ui/src/lib/externalPlayer.ts";
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
const auth = {
  signIn: (backend: BackendConfig, email: string, password: string) =>
    invoke<unknown>("auth.configureBackend", {
      url: backend.url,
      key: backend.key,
      selfHosted: backend.selfHosted,
    }).then(() => invoke<Session>("auth.signIn", { email, password })),
  // Restores rather than reports. `auth.state` answers with whatever session
  // is loaded, which on a fresh launch is none — the shell has to be asked to
  // rotate the stored credential first.
  restore: () => invoke<Session>("auth.restore"),
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
      mediaId: source.mediaId,
      startPositionMs: source.startPositionMs ?? 0,
      requestHeaders: source.requestHeaders ?? {},
      // How the shell files what was watched. Without it playback works and
      // the history does not.
      progress: source.progress,
    }).then(() => undefined),
  stop: () => invoke<unknown>("player.stop").then(() => undefined),
};

export const platform: Platform = {
  auth,
  player,
  downloads,
  // Absent until the resolver is ported. The contract exists, the shell's
  // credentials do too, but nothing on either client turns a cached link into
  // a playable URL yet — so the UI should go on rendering as though it cannot,
  // because it cannot.
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
  // Reused for now, and only partly right: these launch by URL scheme, which a
  // webview answers differently from a browser tab. What a desktop shell should
  // do instead is ask the operating system what is installed — that belongs in
  // Rust, and replaces this wholesale rather than patching it.
  externalPlayer: {
    options: externalPlayerOptions,
    label: externalPlayerLabel,
    isAvailable: isExternalPlayerAvailable,
    launch: launchExternalPlayer,
    copyUrl: copyStreamUrl,
  },
};

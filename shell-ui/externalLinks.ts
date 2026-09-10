import { invoke } from "./bridge.ts";

/**
 * WebView2 does not create a browser tab for `window.open(..., "_blank")`.
 * Route those external HTTP(S) links through the shell, which opens the
 * viewer's configured default browser. Other targets retain normal WebView
 * behavior in case the shared UI ever uses them for internal navigation.
 */
export function installExternalLinkBridge() {
  const webviewOpen = window.open.bind(window);

  window.open = ((url?: string | URL, target?: string, features?: string) => {
    const rawUrl = url?.toString();
    if (rawUrl && target === "_blank") {
      const parsed = new URL(rawUrl, window.location.href);
      if (parsed.protocol === "http:" || parsed.protocol === "https:") {
        void invoke("system.openExternal", { url: parsed.href }).catch((error) => {
          console.error("Could not open external link", error);
        });
        return null;
      }
    }

    return webviewOpen(url, target, features);
  }) as typeof window.open;
}

import type { BridgeEvent, BridgeRequest, BridgeResponse } from "./types";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: {
      invoke(command: string, args?: unknown): Promise<unknown>;
    };
    __NUVIO_BRIDGE_DELIVER__?: (message: BridgeResponse | BridgeEvent) => void;
  }
}

/** Tauri's own invoke, reached through the internals object so the app does not
 *  have to depend on `@tauri-apps/api` for a single call. */
function tauriInvoke(command: string, args: unknown): Promise<unknown> {
  const internals = window.__TAURI_INTERNALS__;
  if (!internals) {
    return Promise.reject(
      new Error("Native bridge is unavailable. Run the Rust shell, not the Vite page."),
    );
  }
  return internals.invoke(command, args);
}

type PendingCall = {
  resolve(value: unknown): void;
  reject(reason: Error): void;
};

type EventListener = (payload: unknown) => void;

const pending = new Map<string, PendingCall>();
const listeners = new Map<string, Set<EventListener>>();
let nextRequestId = 1;

window.__NUVIO_BRIDGE_DELIVER__ = (message) => {
  if ("event" in message) {
    listeners.get(message.event)?.forEach((listener) => listener(message.payload));
    return;
  }

  const call = pending.get(message.id);
  if (!call) return;
  pending.delete(message.id);

  if (message.ok) {
    call.resolve(message.result);
  } else {
    call.reject(new Error(message.error?.message ?? "Native request failed"));
  }
};

export function invoke<T>(method: string, params: unknown = {}): Promise<T> {
  const request: BridgeRequest = {
    id: String(nextRequestId++),
    method,
    params,
  };

  const settled = new Promise<T>((resolve, reject) => {
    pending.set(request.id, {
      resolve: (value) => resolve(value as T),
      reject,
    });
  });

  // The command returns the batch this request produced — a response plus any
  // events — rather than the shell pushing them in via `evaluate_script`. They
  // still go through the same deliver path so id matching and event fan-out are
  // unchanged.
  tauriInvoke("bridge", { raw: JSON.stringify(request) })
    .then((messages) => {
      for (const message of (messages ?? []) as Array<BridgeResponse | BridgeEvent>) {
        window.__NUVIO_BRIDGE_DELIVER__?.(message);
      }
    })
    .catch((error: unknown) => {
      const call = pending.get(request.id);
      if (!call) return;
      pending.delete(request.id);
      call.reject(error instanceof Error ? error : new Error(String(error)));
    });

  return settled;
}

export function onNativeEvent<T>(event: string, listener: (payload: T) => void): () => void {
  const untyped = listener as EventListener;
  const eventListeners = listeners.get(event) ?? new Set<EventListener>();
  eventListeners.add(untyped);
  listeners.set(event, eventListeners);

  return () => {
    eventListeners.delete(untyped);
    if (eventListeners.size === 0) listeners.delete(event);
  };
}


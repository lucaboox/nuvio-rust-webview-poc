import type { BridgeEvent, BridgeRequest, BridgeResponse } from "./types";

declare global {
  interface Window {
    ipc?: { postMessage(message: string): void };
    __NUVIO_BRIDGE_DELIVER__?: (message: BridgeResponse | BridgeEvent) => void;
  }
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
  if (!window.ipc) {
    return Promise.reject(new Error("Native bridge is unavailable. Run the Rust shell, not the Vite page."));
  }

  const request: BridgeRequest = {
    id: String(nextRequestId++),
    method,
    params,
  };

  return new Promise<T>((resolve, reject) => {
    pending.set(request.id, {
      resolve: (value) => resolve(value as T),
      reject,
    });
    window.ipc?.postMessage(JSON.stringify(request));
  });
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


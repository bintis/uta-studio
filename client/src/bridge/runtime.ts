/** Tauri-only IPC boundary for Uta Studio. */
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export type EventCallback<T> = (event: { payload: T }) => void;

export { Channel, invoke, listen };
export type { UnlistenFn };

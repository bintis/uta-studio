import { convertFileSrc as tauriConvertFileSrc } from "@tauri-apps/api/core";

/**
 * Tauri exposes local filesystem paths to the webview via `asset://`.
 */
export const convertFileSrc = (path: string): string => {
  if (!path) return "";
  return tauriConvertFileSrc(path);
};

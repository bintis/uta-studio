import { openPath as tauriOpenPath, openUrl as tauriOpenUrl } from "@tauri-apps/plugin-opener";
import { invoke, isTauri } from "./runtime";

export const openUrl = async (url: string): Promise<void> => {
  if (isTauri) {
    await tauriOpenUrl(url);
    return;
  }

  window.open(url, "_blank", "noopener,noreferrer");
};

export const openLog = async (): Promise<void> => {
  if (isTauri) {
    const path = await invoke<string>("get_log_path");
    await tauriOpenPath(path);
    return;
  }

  window.open("/uta-studio.log", "_blank", "noopener,noreferrer");
};

export const getRecentLogs = async (): Promise<string[]> => {
  if (!isTauri) return [];
  return await invoke<string[]>("get_recent_logs");
};

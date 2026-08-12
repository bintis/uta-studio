import { exit as tauriExit } from "@tauri-apps/plugin-process";
import { isTauri } from "./runtime";

/**
 * Whether the host has a meaningful "quit the app" affordance. Tauri can
 * terminate the process. A plain browser preview has no reliable equivalent,
 * so hide exit-related UI when this is false.
 */
export const EXIT_SUPPORTED: boolean = isTauri;

export const exit = async (exitCode: 0 | 1 = 0) => {
  if (isTauri) {
    await tauriExit(exitCode);
    return;
  }
  // Browsers can't terminate the page programmatically; closing the tab is
  // the closest analogue and only works for windows opened via script.
  window.close();
};

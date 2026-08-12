import { invoke, isTauri } from "./runtime";

export const triggerFrontendReady = async (): Promise<void> => {
  if (!isTauri) return;
  return await invoke<void>("frontend_ready");
};

export const windowImmersive = async (): Promise<boolean> => {
  if (!isTauri) {
    return typeof document !== "undefined" && document.fullscreenElement != null;
  }
  return invoke<boolean>("window_immersive");
};

export const minimizeWindow = async (): Promise<void> => {
  if (!isTauri) return;
  return invoke<void>("minimize_window");
};

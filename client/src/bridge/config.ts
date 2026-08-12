import { AppConfig } from "@/types/AppConfig";
import { invoke } from "./runtime";

export function getPreloadedConfig(): AppConfig | undefined {
  if (typeof window === "undefined") {
    return undefined;
  }

  return window.__UTA_STUDIO_APP_CONFIG__;
}

export const loadConfig = async (): Promise<AppConfig> => {
  return await invoke<AppConfig>("load_config");
};

export const saveConfig = async (config: AppConfig): Promise<AppConfig> =>
  invoke<AppConfig>("save_config", { config });

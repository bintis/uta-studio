import { open } from "@tauri-apps/plugin-dialog";

import type { AppConfig } from "@/types/AppConfig";
import type { LibraryFolderEntry } from "@/types/LibraryFolderEntry";
import type { LibrarySource } from "@/types/LibrarySource";
import { invoke } from "./runtime";

/** Select a local folder on the Studio machine. */
export const selectFolderPath = async (): Promise<string | undefined> => {
  const folder = await open({ directory: true, multiple: false });
  return folder ?? undefined;
};

/** Select a default folder for export dialogs without changing the library. */
export const selectExportFolderPath = selectFolderPath;

export const triggerScan = async (): Promise<void> => {
  await invoke("trigger_scan");
};

export const setLibrarySource = async (source: LibrarySource): Promise<AppConfig> => {
  return await invoke<AppConfig>("set_library_source", { source });
};

export const addLibraryFolder = async (path: string): Promise<AppConfig> =>
  invoke<AppConfig>("add_library_folder", { path });

export const removeLibraryFolder = async (path: string): Promise<AppConfig> =>
  invoke<AppConfig>("remove_library_folder", { path });

export const listLibraryFolder = async (path: string): Promise<LibraryFolderEntry[]> =>
  invoke<LibraryFolderEntry[]>("list_library_folder", { path });

export const openLibraryEntry = async (path: string): Promise<void> =>
  invoke<void>("open_library_entry", { path });

export const revealLibraryEntry = async (path: string): Promise<void> =>
  invoke<void>("reveal_library_entry", { path });

export const clearLibrarySource = async (): Promise<AppConfig> => {
  return await invoke<AppConfig>("clear_library_source");
};

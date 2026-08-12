import { Channel, invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import type { Song } from "@/types/Song";
import type { ExportProgress } from "@/types/ExportProgress";
import type { AppConfig } from "@/types/AppConfig";

const safeFilename = (value: string): string =>
  Array.from(value.normalize("NFKC"), (character) =>
    character.charCodeAt(0) < 32 ? "_" : character,
  )
    .join("")
    .replace(/[\\/:*?"<>|]/g, "_")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 120) || "song";

const defaultExportPath = (config: AppConfig | undefined, filename: string): string => {
  const folder = config?.export_path?.replace(/[\\/]+$/, "");
  return folder ? `${folder}/${filename}` : filename;
};

/** Ask for a destination and export one fully analysed song as a portable
 * `.utz` package. Returns null when the user cancels. */
export async function exportSongPackage(
  song: Song,
  onProgress?: (progress: ExportProgress) => void,
  config?: AppConfig,
): Promise<string | null> {
  const output = await save({
    title: "Export Uta song package",
    defaultPath: defaultExportPath(config, `${safeFilename(`${song.artist} - ${song.title}`)}.utz`),
    filters: [{ name: "Uta song package", extensions: ["utz"] }],
  });
  if (!output) return null;
  const onEvent = new Channel<ExportProgress>();
  onEvent.onmessage = (progress) => onProgress?.(progress);
  return await invoke<string>("export_utz", { fileHash: song.file_hash, output, onEvent });
}

/** Export a UTF-8 UltraStar 1.1 chart and copy its referenced media beside it. */
export async function exportUltraStar(song: Song, config?: AppConfig): Promise<string | null> {
  const output = await save({
    title: "Export UltraStar song",
    defaultPath: defaultExportPath(config, `${safeFilename(`${song.artist} - ${song.title}`)}.txt`),
    filters: [{ name: "UltraStar song", extensions: ["txt"] }],
  });
  if (!output) return null;
  return await invoke<string>("export_ultrastar", { fileHash: song.file_hash, output });
}

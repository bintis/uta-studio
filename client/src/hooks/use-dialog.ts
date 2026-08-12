import { Song } from "@/types/Song";
import { atom, useAtom } from "jotai";

export type ClearCacheTarget = "all" | "models";

export type DialogMode =
  | "exit"
  | "about"
  | { mode: "language"; song: Song }
  | { mode: "edit-lyrics"; song: Song }
  | { mode: "clear-cache"; target: ClearCacheTarget }
  | null;

const dialogAtom = atom<DialogMode>(null);

export const useDialog = () => {
  const [mode, setMode] = useAtom(dialogAtom);

  return {
    mode,
    setMode,
    close() {
      setMode(null);
    },
  };
};

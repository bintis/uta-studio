import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useSetAtom } from "jotai";
import { toast } from "sonner";

import {
  addLibraryFolder,
  clearLibrarySource,
  removeLibraryFolder,
  selectFolderPath,
  triggerScan,
} from "@/bridge/source";
import { libraryFilterAtom } from "@/hooks/use-library-filter";
import { searchAtom } from "@/hooks/use-search";
import { EMPTY_LIBRARY_FILTER } from "@/lib/library-menu-filter";
import { ANALYSIS_QUEUE, CONFIG, MENU, SONGS, SONGS_META } from "@/queries/keys";
import type { AppConfig } from "@/types/AppConfig";

const useInvalidateLibrary = () => {
  const queryClient = useQueryClient();
  return () => {
    for (const queryKey of [CONFIG, SONGS, SONGS_META, MENU, ANALYSIS_QUEUE]) {
      void queryClient.invalidateQueries({ queryKey });
    }
  };
};

const useResetLibraryNavigation = () => {
  const setLibraryFilter = useSetAtom(libraryFilterAtom);
  const setSearch = useSetAtom(searchAtom);
  return () => {
    setLibraryFilter(EMPTY_LIBRARY_FILTER);
    setSearch("");
  };
};

export const useSelectFolderSource = () => {
  const queryClient = useQueryClient();
  const invalidateLibrary = useInvalidateLibrary();
  const resetNavigation = useResetLibraryNavigation();

  return useMutation({
    mutationFn: async (): Promise<AppConfig | null> => {
      const path = await selectFolderPath();
      return path ? addLibraryFolder(path) : null;
    },
    onSuccess: (config) => {
      if (!config) return;
      queryClient.setQueryData(CONFIG, config);
      resetNavigation();
      invalidateLibrary();
    },
    onError: (error: Error) => toast.error(`Failed to select folder: ${error.message}`),
  });
};

export const useRemoveFolderSource = () => {
  const queryClient = useQueryClient();
  const invalidateLibrary = useInvalidateLibrary();
  const resetNavigation = useResetLibraryNavigation();

  return useMutation({
    mutationFn: removeLibraryFolder,
    onSuccess: (config) => {
      queryClient.setQueryData(CONFIG, config);
      resetNavigation();
      invalidateLibrary();
    },
    onError: (error: Error) => toast.error(`Could not remove folder: ${error.message}`),
  });
};

export const useRescan = () => {
  const invalidateLibrary = useInvalidateLibrary();
  return useMutation({
    mutationFn: triggerScan,
    onSuccess: invalidateLibrary,
    onError: (error: Error) => toast.error(`Rescan failed: ${error.message}`),
  });
};

export const useDisconnectSource = () => {
  const queryClient = useQueryClient();
  const invalidateLibrary = useInvalidateLibrary();
  const resetNavigation = useResetLibraryNavigation();
  return useMutation({
    mutationFn: clearLibrarySource,
    onSuccess: (config) => {
      queryClient.setQueryData(CONFIG, config);
      resetNavigation();
      invalidateLibrary();
    },
    onError: (error: Error) => toast.error(`Could not clear folder: ${error.message}`),
  });
};

import { useConfig } from "@/queries/use-config";
import {
  useDisconnectSource,
  useRemoveFolderSource,
  useRescan,
  useSelectFolderSource,
} from "@/mutations/use-source-mutations";

export const useLibrarySourceActions = () => {
  const { data: config } = useConfig();
  const folderMutation = useSelectFolderSource();
  const rescanMutation = useRescan();
  const disconnectMutation = useDisconnectSource();
  const removeFolderMutation = useRemoveFolderSource();
  const paths = config?.library_source?.kind === "folders" ? config.library_source.paths : [];
  const hasSource = paths.length > 0;

  return {
    config,
    paths,
    hasSource,
    isFolderSource: hasSource,
    selectFolder: () => folderMutation.mutate(),
    rescan: () => rescanMutation.mutate(),
    disconnectSource: () => disconnectMutation.mutate(),
    removeFolder: (path: string) => removeFolderMutation.mutate(path),
    isPending:
      folderMutation.isPending ||
      rescanMutation.isPending ||
      disconnectMutation.isPending ||
      removeFolderMutation.isPending,
    rescanDisabled: !hasSource || rescanMutation.isPending,
  };
};

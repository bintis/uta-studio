import type { ComponentType, SVGProps } from "react";
import { FolderIcon, RefreshCwIcon } from "lucide-react";

import type { BadgeTone } from "@/components/menu/sidebar/source-action-button";
import { useLibrarySourceActions } from "@/hooks/use-library-source-actions";

export interface SourceButton {
  key: string;
  icon: ComponentType<SVGProps<SVGSVGElement>>;
  label: string;
  tooltip: string;
  handler: () => void;
  disabled: boolean;
  badge?: BadgeTone;
}

export const useSourceButtons = (): SourceButton[] => {
  const { selectFolder, rescan, rescanDisabled, isPending, hasSource } = useLibrarySourceActions();
  const buttons: SourceButton[] = [
    {
      key: "folder",
      icon: FolderIcon,
      label: hasSource ? "Change folder" : "Select folder",
      tooltip: hasSource ? "Change local source folder" : "Select local source folder",
      handler: selectFolder,
      disabled: isPending,
    },
  ];
  if (hasSource) {
    buttons.push({
      key: "rescan",
      icon: RefreshCwIcon,
      label: "Rescan folder",
      tooltip: "Rescan local folder",
      handler: rescan,
      disabled: rescanDisabled,
    });
  }
  return buttons;
};

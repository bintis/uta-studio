import { EXIT_SUPPORTED } from "@/bridge/exit";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useDialog } from "@/hooks/use-dialog";
import { DoorOpenIcon, InfoIcon } from "lucide-react";
import logoUrl from "../../../../client/src-tauri/icons/icon.png";

export const AppMenu = () => {
  const { setMode } = useDialog();

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          className="ml-1 inline-flex size-8 items-center justify-center rounded-full outline-none transition-opacity hover:opacity-75 focus-visible:ring-1 focus-visible:ring-foreground/22"
          aria-label="Uta Studio menu"
          title="Uta Studio"
        >
          <img
            src={logoUrl}
            alt=""
            className="size-7 rounded-full object-cover shadow-sm ring-1 ring-border/55"
          />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={9} className="min-w-52">
        <DropdownMenuLabel>
          <span className="block text-xs font-medium text-foreground">Uta Studio</span>
          <span className="mt-0.5 block text-[9px] font-normal text-muted-foreground">
            Karaoke authoring workspace
          </span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem onClick={() => setMode("about")}>
          <InfoIcon /> About Uta Studio
        </DropdownMenuItem>
        {EXIT_SUPPORTED ? (
          <DropdownMenuItem onClick={() => setMode("exit")}>
            <DoorOpenIcon /> Exit
          </DropdownMenuItem>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  );
};

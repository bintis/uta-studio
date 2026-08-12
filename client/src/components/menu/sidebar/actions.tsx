import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import logoUrl from "../../../../../client/src-tauri/icons/icon.png";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem } from "@/components/ui/sidebar";
import { EXIT_SUPPORTED } from "@/bridge/exit";
import { useMenuFocus } from "@/contexts/menu-focus-context";
import { useDialog } from "@/hooks/use-dialog";
import { useIsMobile } from "@/hooks/use-is-mobile";
import { useNavInput } from "@/hooks/navigation/use-nav-input";
import { ChevronsUpDownIcon, DoorOpenIcon, InfoIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

interface ActionsProps {
  registerCallback: (callback: (() => void) | null) => void;
  focusedSidebarIndex: number;
}

export const Actions = ({ registerCallback, focusedSidebarIndex }: ActionsProps) => {
  const { setMode } = useDialog();
  const { focus, actionsRef } = useMenuFocus();
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const dropdownOpenRef = useRef(false);
  const isMobile = useIsMobile();

  dropdownOpenRef.current = dropdownOpen;

  useEffect(() => {
    registerCallback(() => {
      setDropdownOpen(true);
      setTimeout(() => {
        const firstItem = document.querySelector('[role="menu"] [role="menuitem"]');
        if (firstItem instanceof HTMLElement) firstItem.focus();
      }, 50);
    });
    actionsRef.current.onSidebarBack = () => {
      if (!dropdownOpenRef.current) return false;
      setDropdownOpen(false);
      return true;
    };
    actionsRef.current.isSidebarBusy = () => dropdownOpenRef.current;

    return () => {
      registerCallback(null);
      actionsRef.current.onSidebarBack = null;
      actionsRef.current.isSidebarBusy = null;
    };
  }, [registerCallback, actionsRef]);

  useNavInput(
    useCallback((action) => {
      if (!dropdownOpenRef.current) return;
      const focused = document.activeElement;
      if (action.up || action.down) {
        focused?.dispatchEvent(
          new KeyboardEvent("keydown", {
            key: action.up ? "ArrowUp" : "ArrowDown",
            bubbles: true,
          }),
        );
      }
      if (action.confirm && focused instanceof HTMLElement) focused.click();
    }, []),
  );

  const isSidebarActive = focus.active && focus.panel === "sidebar";

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu open={dropdownOpen} onOpenChange={setDropdownOpen}>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              tabIndex={-1}
              size="lg"
              data-sidebar-nav-index={focusedSidebarIndex}
              className={`h-10 data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground hover:ring-0 ${
                isSidebarActive && focus.sidebarIndex === focusedSidebarIndex
                  ? "ring-2 ring-primary bg-sidebar-accent"
                  : ""
              }`}
            >
              <Avatar>
                <AvatarImage src={logoUrl} alt="" className="object-cover" />
                <AvatarFallback>US</AvatarFallback>
              </Avatar>
              <span className="truncate font-medium">Uta Studio</span>
              <ChevronsUpDownIcon className="ml-auto size-4" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            side={isMobile ? "top" : "right"}
            align={isMobile ? "start" : "end"}
            collisionPadding={8}
            className="min-w-56"
          >
            <DropdownMenuGroup>
              <DropdownMenuItem onClick={() => setMode("about")}>
                <InfoIcon /> About Uta Studio
              </DropdownMenuItem>
              {EXIT_SUPPORTED && (
                <DropdownMenuItem onClick={() => setMode("exit")}>
                  <DoorOpenIcon /> Exit
                </DropdownMenuItem>
              )}
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  );
};

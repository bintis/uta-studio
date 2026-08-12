import { SidebarMenu, SidebarMenuButton, SidebarMenuItem } from "@/components/ui/sidebar";
import { useMenuFocus } from "@/contexts/menu-focus-context";
import { useDialog, type ClearCacheTarget } from "@/hooks/use-dialog";
import { BoxIcon, Trash2Icon, type LucideIcon } from "lucide-react";
import { useEffect, useMemo, useRef } from "react";

interface CacheButton {
  icon: LucideIcon;
  label: string;
  target: ClearCacheTarget;
}

interface CacheActionsProps {
  focusedSidebarIndex: number;
  registerCallback: (callback: ((subIndex: number) => void) | null) => void;
}

export const CacheActions = ({ focusedSidebarIndex, registerCallback }: CacheActionsProps) => {
  const { focus, actionsRef } = useMenuFocus();
  const { setMode } = useDialog();

  const buttons = useMemo<CacheButton[]>(
    () => [
      { icon: Trash2Icon, label: "Clear all cache", target: "all" },
      { icon: BoxIcon, label: "Clear models cache", target: "models" },
    ],
    [],
  );

  const buttonsRef = useRef(buttons);
  buttonsRef.current = buttons;
  const setModeRef = useRef(setMode);
  setModeRef.current = setMode;

  useEffect(() => {
    const map = actionsRef.current.sidebarSubCountByIndex;
    map.set(focusedSidebarIndex, buttons.length);

    registerCallback((subIndex: number) => {
      const button = buttonsRef.current[subIndex];
      if (!button) return;
      setModeRef.current({ mode: "clear-cache", target: button.target });
    });

    return () => {
      map.delete(focusedSidebarIndex);
      registerCallback(null);
    };
  }, [actionsRef, focusedSidebarIndex, registerCallback, buttons.length]);

  const isSidebarActive = focus.active && focus.panel === "sidebar";
  const isClusterFocused = isSidebarActive && focus.sidebarIndex === focusedSidebarIndex;

  return (
    <SidebarMenu data-sidebar-nav-index={focusedSidebarIndex}>
      {buttons.map((button, index) => {
        const Icon = button.icon;
        const isButtonFocused = isClusterFocused && focus.sidebarSubIndex === index;

        return (
          <SidebarMenuItem key={button.label}>
            <SidebarMenuButton
              tabIndex={-1}
              data-sidebar-sub-index={index}
              className={`rounded-none text-sidebar-foreground/58 hover:bg-transparent hover:text-sidebar-foreground ${
                isButtonFocused ? "bg-transparent text-primary ring-0" : ""
              }`}
              onClick={() => setMode({ mode: "clear-cache", target: button.target })}
            >
              <Icon />
              <span>{button.label}</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        );
      })}
    </SidebarMenu>
  );
};

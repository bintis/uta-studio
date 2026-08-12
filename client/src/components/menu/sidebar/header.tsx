import { SidebarHeader } from "@/components/ui/sidebar";

import { Settings } from "lucide-react";
import { useNavigate } from "react-router";
import { ThemeToggle } from "./theme-toggle";

interface HeaderProps {
  focusedSidebarIndex: number;
  registerCallback: (callback: (() => void) | null) => void;
}

export const Header = ({ focusedSidebarIndex, registerCallback }: HeaderProps) => {
  const navigate = useNavigate();

  return (
    <SidebarHeader className="px-3 pb-5 pt-4">
      <div className="relative flex w-full items-center gap-1.5">
        <button
          type="button"
          className="text-[1.55rem] font-extralight leading-none tracking-[-0.06em] text-sidebar-foreground"
          onClick={() => navigate("/")}
          aria-label="Uta Studio library"
        >
          uta
        </button>
        <button
          type="button"
          className="ml-auto inline-flex size-6 items-center justify-center rounded-full text-sidebar-foreground/44 hover:bg-sidebar-accent hover:text-sidebar-foreground"
          onClick={() => navigate("/settings")}
          aria-label="Settings"
        >
          <Settings className="size-3.5" />
        </button>
        <ThemeToggle
          className="shrink-0"
          focusedSidebarIndex={focusedSidebarIndex}
          registerCallback={registerCallback}
        />
      </div>
    </SidebarHeader>
  );
};

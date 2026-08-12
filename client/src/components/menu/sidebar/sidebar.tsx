import { Sidebar as ShadCnSidebar, SidebarProvider } from "@/components/ui/sidebar";
import { Header } from "./header";
import { MainNavigation } from "./main-navigation";
import { useMenuFocus } from "@/contexts/menu-focus-context";
import { useCallback, useEffect, useRef, useState, type PropsWithChildren } from "react";

const THEME_SLOT_INDEX = 0;
const MAIN_NAV_BASE_INDEX = THEME_SLOT_INDEX + 1;

type SidebarCallback = () => void;

export const Sidebar = ({ children }: PropsWithChildren<{}>) => {
  const { actionsRef, setFocus } = useMenuFocus();
  const [mainNavigationCallbacks, setMainNavigationCallbacks] = useState<SidebarCallback[]>([]);

  const themeCallbackRef = useRef<SidebarCallback | null>(null);

  const sidebarCount = MAIN_NAV_BASE_INDEX + mainNavigationCallbacks.length;

  const registerThemeCallback = useCallback((callback: (() => void) | null) => {
    themeCallbackRef.current = callback;
  }, []);

  const registerMainNavigationCallbacks = useCallback((callbacks: SidebarCallback[]) => {
    setMainNavigationCallbacks(callbacks);
  }, []);

  const confirmSidebarSlot = useCallback(
    (index: number) => {
      if (index === THEME_SLOT_INDEX) {
        themeCallbackRef.current?.();
        return;
      }

      if (index >= MAIN_NAV_BASE_INDEX && index < sidebarCount) {
        mainNavigationCallbacks[index - MAIN_NAV_BASE_INDEX]?.();
      }
    },
    [mainNavigationCallbacks, sidebarCount],
  );

  const clampSidebarFocus = useCallback(() => {
    setFocus((prev) => {
      const sidebarIndex = Math.min(prev.sidebarIndex, sidebarCount - 1);
      return sidebarIndex === prev.sidebarIndex ? prev : { ...prev, sidebarIndex };
    });
  }, [setFocus, sidebarCount]);

  useEffect(() => {
    actionsRef.current.sidebarCount = sidebarCount;
    actionsRef.current.onConfirmSidebar = confirmSidebarSlot;
    clampSidebarFocus();

    return () => {
      actionsRef.current.onConfirmSidebar = null;
      actionsRef.current.sidebarCount = 0;
    };
  }, [actionsRef, clampSidebarFocus, confirmSidebarSlot, sidebarCount]);

  return (
    <SidebarProvider>
      <ShadCnSidebar
        variant="sidebar"
        className="roon-sidebar [&_[data-sidebar=sidebar]]:backdrop-blur-2xl"
      >
        <Header focusedSidebarIndex={THEME_SLOT_INDEX} registerCallback={registerThemeCallback} />

        <MainNavigation
          baseIndex={MAIN_NAV_BASE_INDEX}
          registerCallbacks={registerMainNavigationCallbacks}
        />
      </ShadCnSidebar>
      {children}
    </SidebarProvider>
  );
};

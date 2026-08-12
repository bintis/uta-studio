import { ClearCacheDialog } from "@/components/menu/dialogs/clear-cache";
import { EditLyricsDialog } from "@/components/menu/dialogs/edit-lyrics";
import { ExitDialog } from "@/components/menu/dialogs/exit";
import { InfoDialog } from "@/components/menu/dialogs/info";
import { SelectLanguageDialog } from "@/components/menu/dialogs/language";
import { Sidebar } from "@/components/menu/sidebar/sidebar";
import { AppTopBar } from "@/components/menu/app-top-bar";
import { EmptySongList } from "@/components/menu/song-list/empty-song-list";
import { SongList } from "@/components/menu/song-list/song-list";
import { SidebarInset } from "@/components/ui/sidebar";
import { EXIT_SUPPORTED } from "@/bridge/exit";
import { useMenuNav } from "@/hooks/navigation/use-menu-nav";
import { useDialog } from "@/hooks/use-dialog";
import { useShouldRunSetup } from "@/hooks/use-should-run-setup";
import { useSongsMeta } from "@/queries/use-songs";
import { useCallback } from "react";
import { Outlet, useLocation } from "react-router";

export const MenuIndex = () => {
  const { data: meta, isLoading: isLoadingMeta } = useSongsMeta();

  if (isLoadingMeta) {
    return null;
  }

  if (meta?.folder) {
    return <SongList />;
  }

  return <EmptySongList />;
};

export const MenuLayout = () => {
  const { mode, setMode } = useDialog();
  const { shouldRunSetup } = useShouldRunSetup();
  const location = useLocation();

  const isContentPage = location.pathname !== "/" && location.pathname !== "/song";
  const overlayOpen = isContentPage || mode !== null || shouldRunSetup;

  const onBack = useCallback(() => {
    setMode((prev) => {
      if (prev === null) {
        // Web mode has no app to exit; swallow the back input rather than
        // surfacing a dialog whose confirm action can't do anything useful.
        return EXIT_SUPPORTED ? "exit" : null;
      }

      if (prev === "exit") {
        return null;
      }

      return prev;
    });
  }, [setMode]);

  useMenuNav({ overlayOpen, onBack });

  return (
    <Sidebar>
      {EXIT_SUPPORTED && <ExitDialog />}
      <InfoDialog />
      <SelectLanguageDialog />
      <EditLyricsDialog />
      <ClearCacheDialog />
      <SidebarInset>
        <AppTopBar />
        <div className="flex min-h-0 flex-1 overflow-hidden">
          <Outlet />
        </div>
      </SidebarInset>
    </Sidebar>
  );
};

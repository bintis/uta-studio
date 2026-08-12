import { BrowserRouter, Routes, Route } from "react-router";
import { lazy, Suspense } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "./App.css";
import { Toaster } from "./components/ui/sonner";
import { TauriAppShell } from "./components/window/title-bar";
import { NavInputProvider } from "./contexts/nav-input-context";
import { MenuFocusProvider } from "./contexts/menu-focus-context";
import { MenuIndex, MenuLayout } from "./pages/menu/menu";
import { ThemeProvider } from "./contexts/theme-context";
import { useConfig } from "./queries/use-config";
import { TooltipProvider } from "./components/ui/tooltip";
import { LoaderCircle } from "lucide-react";
import { Setup } from "./components/menu/dialogs/setup";
import { SettingsPage } from "./pages/menu/settings";

const ChartEditor = lazy(() =>
  import("./pages/editor/chart-editor").then((module) => ({ default: module.ChartEditor })),
);
const SongDetailPage = lazy(() =>
  import("./pages/menu/song-detail").then((module) => ({ default: module.SongDetailPage })),
);
const FoldersPage = lazy(() =>
  import("./pages/menu/folders").then((module) => ({ default: module.FoldersPage })),
);

const PageLoader = () => (
  <div className="bg-editor-ambient grid h-full place-items-center" aria-label="Loading workspace">
    <LoaderCircle className="size-5 animate-spin text-primary" />
  </div>
);

const queryClient = new QueryClient();

const InnerWrapper = () => (
  <>
    <MenuFocusProvider>
      <BrowserRouter>
        <Routes>
          <Route path="/" element={<MenuLayout />}>
            <Route index element={<MenuIndex />} />
            <Route
              path="song"
              element={
                <Suspense fallback={<PageLoader />}>
                  <SongDetailPage />
                </Suspense>
              }
            />
            <Route
              path="folders"
              element={
                <Suspense fallback={<PageLoader />}>
                  <FoldersPage />
                </Suspense>
              }
            />
            <Route path="settings" element={<SettingsPage />} />
          </Route>
          <Route
            path="/editor"
            element={
              <Suspense fallback={<PageLoader />}>
                <ChartEditor />
              </Suspense>
            }
          />
        </Routes>
      </BrowserRouter>
    </MenuFocusProvider>
    <Setup />
    <Toaster />
  </>
);

const ThemeWrapper = () => {
  const { data: config } = useConfig();

  return (
    <ThemeProvider defaultTheme={config?.dark_mode === true ? "dark" : "light"}>
      <TooltipProvider>
        <TauriAppShell>
          <InnerWrapper />
        </TauriAppShell>
      </TooltipProvider>
    </ThemeProvider>
  );
};

const App = () => (
  <NavInputProvider>
    <QueryClientProvider client={queryClient}>
      <ThemeWrapper />
    </QueryClientProvider>
  </NavInputProvider>
);

export default App;

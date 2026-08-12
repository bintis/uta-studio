import { minimizeWindow, triggerFrontendReady, windowImmersive } from "@/bridge/window";
import { isFullScreen as tauriIsFullScreen, setFullScreen } from "@/bridge/fullScreen";
import { loadConfig, saveConfig } from "@/bridge/config";
import { isTauri } from "@/bridge/runtime";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MinusIcon, SquareIcon, XIcon } from "lucide-react";
import { useEffect, useState } from "react";

export function TauriAppShell({ children }: { children: React.ReactNode }) {
  useEffect(() => {
    triggerFrontendReady();
  }, []);

  useEffect(() => {
    if (!isTauri) return;

    const onKeyDown = async (e: KeyboardEvent) => {
      if (e.key !== "F11") return;
      e.preventDefault();

      const [current, config] = await Promise.all([tauriIsFullScreen(), loadConfig()]);

      const next = !current;

      await Promise.all([setFullScreen(next), saveConfig({ ...config, fullscreen: next })]);
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  if (!isTauri) {
    return children;
  }

  return (
    <div className="flex h-svh min-h-0 flex-col">
      <TitleBar />
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{children}</div>
    </div>
  );
}

function TitleBar() {
  const [fullscreen, setFullscreen] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();

    const sync = async () => {
      setFullscreen(await windowImmersive());
    };

    void sync();

    let unlistenResize: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;

    void win
      .onResized(() => {
        void sync();
      })
      .then((fn) => {
        unlistenResize = fn;
      });

    void win
      .onFocusChanged(() => {
        void sync();
      })
      .then((fn) => {
        unlistenFocus = fn;
      });

    return () => {
      unlistenResize?.();
      unlistenFocus?.();
    };
  }, []);

  useEffect(() => {
    document.documentElement.style.setProperty("--titlebar-offset", fullscreen ? "0px" : "2rem");

    return () => {
      document.documentElement.style.removeProperty("--titlebar-offset");
    };
  }, [fullscreen]);

  if (fullscreen) {
    return null;
  }

  const win = getCurrentWindow();

  return (
    <header className="flex h-8 shrink-0 select-none items-stretch border-b border-border/55 bg-background/78 backdrop-blur-xl">
      <div
        className="min-h-0 min-w-0 flex-1"
        aria-hidden
        onMouseDown={() => void win.startDragging()}
      />
      <div className="flex items-center gap-0.5 pl-1 pr-1.5">
        <button
          type="button"
          className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
          aria-label="Minimize"
          onClick={() => void minimizeWindow()}
        >
          <MinusIcon className="size-3.5" strokeWidth={2} />
        </button>
        <button
          type="button"
          className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
          aria-label="Maximize"
          onClick={() => void win.toggleMaximize()}
        >
          <SquareIcon className="size-3" strokeWidth={2} />
        </button>
        <button
          type="button"
          className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-destructive/15 hover:text-destructive"
          aria-label="Close"
          onClick={() => void win.close()}
        >
          <XIcon className="size-3.5" strokeWidth={2} />
        </button>
      </div>
    </header>
  );
}

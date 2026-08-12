import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { useSearch } from "@/hooks/use-search";
import { ChevronLeft, ChevronRight, Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { ActivityCenter } from "./activity-center";
import { AppMenu } from "./app-menu";

export const AppTopBar = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const { search, setSearch } = useSearch();
  const [draft, setDraft] = useState(search);
  const [searchOpen, setSearchOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => setDraft(search), [search]);
  useEffect(() => {
    const onShortcut = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSearchOpen(true);
      }
    };
    window.addEventListener("keydown", onShortcut);
    return () => window.removeEventListener("keydown", onShortcut);
  }, []);

  const commitSearch = (value: string) => {
    setDraft(value);
    setSearch(value);
    if (location.pathname !== "/") navigate("/");
  };

  const onNavigateBack = () => {
    if (["/song", "/settings", "/folders"].includes(location.pathname)) {
      navigate("/");
      return;
    }
    navigate(-1);
  };

  return (
    <>
      <header className="roon-global-bar flex min-h-14 shrink-0 items-center gap-1 border-b border-border/40 px-2 sm:px-3">
        <SidebarTrigger variant="ghost" size="icon" className="md:hidden" />
        <div className="hidden items-center gap-0.5 sm:flex" aria-label="Navigation history">
          <Button
            variant="ghost"
            size="icon-sm"
            className="rounded-full"
            onClick={onNavigateBack}
            aria-label="Back"
          >
            <ChevronLeft />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className="rounded-full"
            onClick={() => navigate(1)}
            aria-label="Forward"
          >
            <ChevronRight />
          </Button>
        </div>
        <div className="flex-1" />
        <ActivityCenter />
        <Popover open={searchOpen} onOpenChange={setSearchOpen}>
          <PopoverTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="relative rounded-full"
              aria-label="Search library"
              title="Search · Ctrl K"
            >
              <Search />
              {search ? (
                <span className="absolute bottom-0.5 size-1 rounded-full bg-primary" />
              ) : null}
            </Button>
          </PopoverTrigger>
          <PopoverContent
            align="end"
            sideOffset={9}
            onOpenAutoFocus={(event) => {
              event.preventDefault();
              searchRef.current?.focus();
              searchRef.current?.select();
            }}
            className="w-[min(92vw,25rem)] gap-0 overflow-hidden rounded-sm bg-popover/96 p-0 shadow-2xl backdrop-blur-2xl"
          >
            <label className="relative block border-b border-border/65">
              <Search className="pointer-events-none absolute left-4 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                ref={searchRef}
                value={draft}
                onChange={(event) => commitSearch(event.target.value)}
                placeholder="Search your library"
                aria-label="Search library"
                className="h-12 rounded-none border-0 bg-transparent pl-11 pr-11 shadow-none focus-visible:ring-0"
              />
              {draft ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  className="absolute right-3 top-3 rounded-full"
                  onClick={() => commitSearch("")}
                  aria-label="Clear search"
                >
                  <X />
                </Button>
              ) : null}
            </label>
            <div className="px-4 py-3 text-[10px] text-muted-foreground">
              {draft ? (
                <>Showing library matches as you type</>
              ) : (
                <>Search tracks, artists, albums, and playlists</>
              )}
            </div>
          </PopoverContent>
        </Popover>
        <AppMenu />
      </header>
    </>
  );
};

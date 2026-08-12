import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useMenuFocus } from "@/contexts/menu-focus-context";
import { useLibraryFilter } from "@/hooks/use-library-filter";
import { usePersistentScroll } from "@/hooks/use-persistent-scroll";
import { useSearch } from "@/hooks/use-search";
import { useConfigMutation } from "@/mutations/use-config-mutation";
import { useConfig } from "@/queries/use-config";
import { useAnalysisQueue, useSongs } from "@/queries/use-songs";
import { useEffect, useMemo, useRef, useState } from "react";
import { Filters, type SongListView } from "./filters";
import { Progress } from "./progress";
import { songKey } from "./shared/song-key";
import type { SongItemProps } from "./types";
import { SongGrid } from "./views/song-grid";
import { SongTable } from "./views/song-table";
import { AlbumArt } from "./shared/album-art";
import { getSongStatusInfo } from "./shared/song-status";
import { SongActionsMenu } from "./song-actions-menu";
import { ArrowRight, FileOutput, LibraryBig, PencilRuler, Sparkles } from "lucide-react";
import { useNavigate } from "react-router";
import { useSongEditorAction } from "@/hooks/use-song-editor-action";

export const SongList = () => {
  const navigate = useNavigate();
  const { data: queue } = useAnalysisQueue();
  const { data: config } = useConfig();
  const { mutate: saveConfig, isPending: isSavingView } = useConfigMutation();
  const { focus, actionsRef, setFocus, selectedSongKeyRef } = useMenuFocus();
  const { setScrollContainer, resetScroll } = usePersistentScroll("songList");
  const { search } = useSearch();
  const { artist, album, playlist, query, status, transcript_source } = useLibraryFilter();
  const { data, fetchNextPage, hasNextPage, isFetchingNextPage, isLoading } = useSongs();
  // Track selection by a rekey-stable key so returning from the full song
  // page restores the highlighted row/card and the scroll position.
  const [selectedKey, setSelectedKey] = useState<string | null>(() => selectedSongKeyRef.current);

  const view: SongListView = config?.song_list_view === "grid" ? "grid" : "table";
  const songs = useMemo(() => data?.pages.flatMap((page) => page.processed) ?? [], [data]);
  const foundSong = songs.find((song) => songKey(song) === selectedKey) ?? null;
  // Keep the last snapshot when a status update temporarily re-sorts the song
  // outside the loaded pages.
  const lastSelectedSongRef = useRef<(typeof songs)[number] | null>(null);
  if (foundSong) {
    lastSelectedSongRef.current = foundSong;
  }
  const selectedSong =
    foundSong ??
    (selectedKey &&
    lastSelectedSongRef.current &&
    songKey(lastSelectedSongRef.current) === selectedKey
      ? lastSelectedSongRef.current
      : null);
  const filterKey = JSON.stringify([
    search,
    artist,
    album,
    playlist,
    query,
    status,
    transcript_source,
  ]);
  const previousFilterKeyRef = useRef(filterKey);
  const songsRef = useRef(songs);
  const sentinelRef = useRef<HTMLDivElement>(null);
  songsRef.current = songs;

  useEffect(() => {
    if (previousFilterKeyRef.current === filterKey) return;
    previousFilterKeyRef.current = filterKey;

    setSelectedKey(null);
    resetScroll();
    setFocus((previous) => ({ ...previous, songIndex: 0 }));
  }, [filterKey, resetScroll, setFocus]);

  useEffect(() => {
    selectedSongKeyRef.current = selectedKey;
  }, [selectedKey, selectedSongKeyRef]);

  useEffect(() => {
    actionsRef.current.songCount = songs.length;
  }, [songs.length, actionsRef]);

  useEffect(() => {
    actionsRef.current.onConfirmSong = (index: number) => {
      const song = songsRef.current[index];
      if (!song) return;

      selectSongRef.current(song);
    };
    return () => {
      actionsRef.current.onConfirmSong = null;
    };
  }, [actionsRef]);

  useEffect(() => {
    const element = sentinelRef.current;
    if (!element) return;

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting && hasNextPage && !isFetchingNextPage) fetchNextPage();
      },
      { rootMargin: "200px" },
    );

    observer.observe(element);
    return () => observer.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage, view]);

  const isSongListActive = focus.active && focus.panel === "songList";
  const hasActiveFilter = Boolean(
    search.trim() || artist || album || playlist || query || status || transcript_source,
  );
  const showEmptyState = songs.length === 0 && !isLoading;
  const selectSong = (song: (typeof songs)[number]) => {
    const key = songKey(song);
    setSelectedKey(key);
    selectedSongKeyRef.current = key;
    navigate("/song", { state: { song } });
  };
  const selectSongRef = useRef(selectSong);
  selectSongRef.current = selectSong;
  const collectionTitle = search.trim()
    ? `Results for “${search.trim()}”`
    : artist
      ? artist
      : album
        ? album
        : playlist
          ? playlist
          : query === "queued"
            ? "Analysis Queue"
            : query === "analysed"
              ? "Completed Charts"
              : query === "videos"
                ? "Video Library"
                : query === "usdx"
                  ? "UltraStar Library"
                  : "Song Library";
  const collectionKind = artist
    ? "Artist"
    : album
      ? "Album"
      : playlist
        ? "Playlist"
        : hasActiveFilter
          ? "Filtered collection"
          : "All music";
  const headerArtwork = songs.slice(0, 4);

  const getItemProps = (song: (typeof songs)[number], index: number): SongItemProps => ({
    song,
    queueStatus: queue?.entries[song.file_hash],
    index,
    isSelected: selectedKey === songKey(song),
    isFocused: isSongListActive && !focus.analyzeAllFocused && focus.songIndex === index,
    onSelect: () => selectSong(song),
  });
  const selectedStatus = selectedSong
    ? getSongStatusInfo(selectedSong, queue?.entries[selectedSong.file_hash])
    : null;
  const selectedAnalysisBusy = Boolean(
    selectedSong &&
    (queue?.entries[selectedSong.file_hash] === "Queued" || selectedStatus?.isAnalyzing),
  );
  const selectedEditorAction = useSongEditorAction(
    selectedSong,
    selectedStatus,
    selectedAnalysisBusy,
  );

  return (
    <div className="roon-library relative flex min-h-0 w-full flex-1 flex-col overflow-hidden">
      {selectedSong?.album_art_path ? (
        <AlbumArt
          song={selectedSong}
          className="pointer-events-none absolute -right-32 top-6 size-[32rem] rounded-full opacity-[0.10] blur-3xl"
          lazy={false}
        />
      ) : null}

      <div className="relative flex min-h-0 flex-1 overflow-hidden">
        <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <header className="roon-commandbar border-b border-border/55 px-4 pb-4 pt-5 sm:px-6 lg:px-7">
            <div className="flex flex-wrap items-center gap-4 xl:flex-nowrap">
              <div className="grid size-20 shrink-0 grid-cols-2 overflow-hidden rounded-sm bg-muted shadow-[0_16px_36px_-24px_rgba(0,0,0,.85)] ring-1 ring-border/60 sm:size-24">
                {headerArtwork.length > 0 ? (
                  headerArtwork.map((song, index) => (
                    <AlbumArt
                      key={song.file_hash}
                      song={song}
                      className="size-full rounded-none"
                      fallbackIconClassName="size-4"
                      lazy={index > 1}
                    />
                  ))
                ) : (
                  <div className="col-span-2 row-span-2 grid place-items-center text-muted-foreground">
                    <LibraryBig className="size-7" />
                  </div>
                )}
              </div>
              <div className="min-w-0 flex-1">
                <p className="text-[8px] font-medium uppercase tracking-[0.18em] text-muted-foreground">
                  {collectionKind}
                </p>
                <h1 className="mt-1 truncate text-2xl font-light tracking-tight sm:text-3xl">
                  {collectionTitle}
                </h1>
                <p className="mt-1 text-[10px] text-muted-foreground">
                  {songs.length} tracks · production workspace
                </p>
              </div>
              <div className="w-full xl:w-auto xl:max-w-[36rem]">
                <Filters
                  view={view}
                  isSavingView={isSavingView}
                  onViewChange={(nextView) => saveConfig({ song_list_view: nextView })}
                />
              </div>
            </div>
          </header>
          <Progress />
          {showEmptyState ? (
            <Empty className="px-4">
              <EmptyHeader>
                <EmptyTitle>{hasActiveFilter ? "No results" : "No songs found"}</EmptyTitle>
                <EmptyDescription>
                  {hasActiveFilter
                    ? "No songs match your search or filters. Try adjusting them."
                    : "This library is empty or still being scanned."}
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <div
              ref={setScrollContainer}
              data-song-layout={view}
              className={cn(
                "song-table-shell min-h-0 max-w-full flex-1 overflow-x-hidden overflow-y-auto",
                view === "table" ? "px-0" : "px-3 py-4 sm:px-5 lg:px-7",
              )}
            >
              {view === "table" ? (
                <SongTable songs={songs} getItemProps={getItemProps} />
              ) : (
                <SongGrid songs={songs} getItemProps={getItemProps} />
              )}
              <div ref={sentinelRef} className="h-1" aria-hidden="true" />
            </div>
          )}
        </main>
      </div>

      <footer className="roon-authoring-dock relative flex min-h-[4.5rem] items-center gap-3 border-t border-border/55 px-3 sm:px-5">
        <div className="flex min-w-0 flex-1 items-center gap-3">
          {selectedSong ? (
            <>
              <AlbumArt song={selectedSong} className="size-11 rounded-sm shadow-lg" lazy={false} />
              <div className="min-w-0">
                <p className="truncate text-xs font-semibold">{selectedSong.title}</p>
                <p className="truncate text-[9px] text-muted-foreground">
                  {selectedSong.artist || "Unknown artist"}
                </p>
              </div>
            </>
          ) : (
            <div className="min-w-0">
              <p className="text-xs font-semibold">Choose a song</p>
              <p className="text-[9px] text-muted-foreground">
                Its production controls appear here
              </p>
            </div>
          )}
        </div>
        <ol
          className="hidden items-center gap-2 text-[9px] font-medium uppercase tracking-[0.12em] text-muted-foreground sm:flex"
          aria-label="Authoring workflow"
        >
          <li className="flex items-center gap-1 text-primary">
            <Sparkles className="size-3" /> Analyze
          </li>
          <ArrowRight className="size-3 opacity-35" aria-hidden="true" />
          <li
            className={
              selectedEditorAction.analyzed
                ? "flex items-center gap-1 text-foreground"
                : "flex items-center gap-1"
            }
          >
            <PencilRuler className="size-3" /> Edit
          </li>
          <ArrowRight className="size-3 opacity-35" aria-hidden="true" />
          <li className="flex items-center gap-1">
            <FileOutput className="size-3" /> Export
          </li>
        </ol>
        {selectedSong && selectedStatus ? (
          <SongActionsMenu
            song={selectedSong}
            status={selectedStatus}
            analysisBusy={selectedAnalysisBusy}
            supportsAnalysisActions={
              selectedSong.is_analyzed && selectedSong.transcript_source !== "Usdx"
            }
            compact
          />
        ) : null}
        <Button
          size="lg"
          className="min-w-32"
          disabled={selectedEditorAction.disabled}
          onClick={selectedEditorAction.trigger}
          title={selectedEditorAction.description}
        >
          <PencilRuler />
          {selectedEditorAction.label}
        </Button>
        {selectedEditorAction.needsSetup ? (
          <Button variant="ghost" size="sm" onClick={selectedEditorAction.openAnalysisSettings}>
            Analysis settings
          </Button>
        ) : null}
      </footer>
    </div>
  );
};

import { ANALYSIS_QUEUE, MENU, SONGS, SONGS_META } from "@/queries/keys";
import { useLibraryFilter } from "@/hooks/use-library-filter";
import { useSearch } from "@/hooks/use-search";
import {
  deleteSongCache,
  enqueueAll,
  enqueueOne,
  realign,
  reanalyzeForceTranscribe,
  reanalyzeFull,
  reanalyzePitch,
  reanalyzeTranscript,
} from "@/bridge/analysis";
import type { Song } from "@/types/Song";
import type { SongsStore } from "@/types/SongsStore";
import { type InfiniteData, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { toast } from "sonner";

const withoutAnalysisCache = (song: Song): Song => ({
  ...song,
  is_analyzed: false,
  language: null,
  transcript_source: null,
  key: null,
  override_key: null,
  tempo: 1,
  key_offset: 0,
  no_stems: false,
  authoring_ready: false,
  authoring_missing: ["instrumental", "transcript", "pitch_track", "pitch_notes"],
  editor_ready: false,
  editor_blocked_reason: null,
});

export const useAnalysis = () => {
  const queryClient = useQueryClient();
  const { artist, album, playlist, query, status, transcript_source } = useLibraryFilter();
  const { search } = useSearch();

  return useMemo(() => {
    const invalidateQueue = () => {
      queryClient.invalidateQueries({ queryKey: ANALYSIS_QUEUE });
    };

    const invalidateSongs = () => {
      queryClient.invalidateQueries({ queryKey: MENU });
      queryClient.invalidateQueries({ queryKey: SONGS });
      queryClient.invalidateQueries({ queryKey: SONGS_META });
      queryClient.invalidateQueries({ queryKey: ANALYSIS_QUEUE });
    };

    const markSongCacheDeleted = (fileHash: string) => {
      queryClient.setQueriesData<InfiniteData<SongsStore>>(
        { queryKey: SONGS },
        (data) =>
          data && {
            ...data,
            pages: data.pages.map((page) => ({
              ...page,
              processed: page.processed.map((song) =>
                song.file_hash === fileHash ? withoutAnalysisCache(song) : song,
              ),
            })),
          },
      );
    };

    const wrap =
      <A extends unknown[]>(handler: (...args: A) => Promise<void>, invalidate: () => void) =>
      async (...args: A) => {
        try {
          await handler(...args);
          invalidate();
        } catch (error: unknown) {
          toast.error(
            `Error while running an analysis action: ${error instanceof Error ? error.message : "unknown error"}`,
          );
        }
      };

    return {
      enqueueOne: wrap(enqueueOne, invalidateQueue),
      enqueueAll: wrap(
        () =>
          enqueueAll({
            artist,
            album,
            playlist,
            query,
            status,
            transcript_source,
            search: search || null,
          }),
        invalidateQueue,
      ),
      deleteSongCache: wrap(async (fileHash: string) => {
        await deleteSongCache(fileHash);
        markSongCacheDeleted(fileHash);
      }, invalidateSongs),
      reanalyzeTranscript: wrap(reanalyzeTranscript, invalidateSongs),
      reanalyzeFull: wrap(reanalyzeFull, invalidateSongs),
      reanalyzePitch: wrap(reanalyzePitch, invalidateSongs),
      realign: wrap(realign, invalidateSongs),
      reanalyzeForceTranscribe: wrap(reanalyzeForceTranscribe, invalidateSongs),
    };
  }, [queryClient, artist, album, playlist, query, status, transcript_source, search]);
};

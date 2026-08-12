import { ANALYSIS_QUEUE, LYRICS, MENU, SONGS, SONGS_META } from "@/queries/keys";
import { saveLyrics } from "@/bridge/lyrics";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

export interface SaveLyricsInput {
  hash: string;
  lines: string[];
  title: string;
}

export const useSaveLyricsMutation = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ hash, lines, title }: SaveLyricsInput) => {
      await saveLyrics(hash, lines);
      toast.info(`Realigning lyrics for "${title}"`);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: LYRICS });
      queryClient.invalidateQueries({ queryKey: MENU });
      queryClient.invalidateQueries({ queryKey: SONGS });
      queryClient.invalidateQueries({ queryKey: SONGS_META });
      queryClient.invalidateQueries({ queryKey: ANALYSIS_QUEUE });
    },
    onError: (error: Error) => {
      toast.error(`Error while saving lyrics: ${error.message}`);
    },
  });
};

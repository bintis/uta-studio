import { CACHE_STATS } from "@/queries/keys";
import { clearAll, clearModels } from "@/bridge/cache";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

export const useClearCache = () => {
  const queryClient = useQueryClient();

  const clearCacheFactory = (handler: () => Promise<void>) => {
    return async () => {
      try {
        await handler();

        queryClient.invalidateQueries({ queryKey: CACHE_STATS });

        toast.info(`Cache was successfully cleared`);
      } catch (error: unknown) {
        toast.error(
          `Error while deleting cache: ${error instanceof Error ? error.message : "unknown error"}`,
        );
      }
    };
  };

  return {
    models: clearCacheFactory(clearModels),
    all: clearCacheFactory(clearAll),
  };
};

import { calculateCacheStats } from "@/bridge/cache";
import { useQuery } from "@tanstack/react-query";
import { CACHE_STATS } from "./keys";

export const useCacheStats = () =>
  useQuery({
    queryKey: CACHE_STATS,
    queryFn: calculateCacheStats,
    refetchInterval: 5000,
  });

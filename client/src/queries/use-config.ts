import { getPreloadedConfig, loadConfig } from "@/bridge/config";
import { useQuery } from "@tanstack/react-query";
import { CONFIG } from "./keys";

export const useConfig = () => {
  const preloaded = getPreloadedConfig();

  return useQuery({
    queryKey: CONFIG,
    queryFn: loadConfig,
    ...(preloaded !== undefined ? { initialData: preloaded } : {}),
  });
};

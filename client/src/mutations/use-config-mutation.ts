import { ANALYSIS_RUNTIME_STATUS, CONFIG } from "@/queries/keys";
import { useConfig } from "@/queries/use-config";
import { loadConfig, saveConfig } from "@/bridge/config";
import { AppConfig } from "@/types/AppConfig";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

export const useConfigMutation = () => {
  const queryClient = useQueryClient();
  const { data: config } = useConfig();

  return useMutation({
    mutationFn: async (partialConfig: Partial<AppConfig>) => {
      const current = queryClient.getQueryData<AppConfig>(CONFIG) ?? config ?? (await loadConfig());
      return saveConfig({ ...current, ...partialConfig });
    },
    onMutate: (partialConfig) => {
      const previous = queryClient.getQueryData<AppConfig>(CONFIG);
      queryClient.setQueryData<AppConfig>(CONFIG, (current) =>
        current ? { ...current, ...partialConfig } : current,
      );
      return { previous };
    },
    onSuccess: (savedConfig) => {
      queryClient.setQueryData(CONFIG, savedConfig);
      queryClient.invalidateQueries({ queryKey: CONFIG });
      queryClient.invalidateQueries({ queryKey: ANALYSIS_RUNTIME_STATUS });
    },
    onError: (error: Error, _partialConfig, context) => {
      if (context?.previous) queryClient.setQueryData(CONFIG, context.previous);
      toast.error(`Error updating the local config: ${error.message}`);
    },
  });
};

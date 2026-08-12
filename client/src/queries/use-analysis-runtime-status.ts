import { loadAnalysisRuntimeStatus } from "@/bridge/setup";
import { useQuery } from "@tanstack/react-query";
import { ANALYSIS_RUNTIME_STATUS } from "./keys";

export const useAnalysisRuntimeStatus = () =>
  useQuery({
    queryKey: ANALYSIS_RUNTIME_STATUS,
    queryFn: loadAnalysisRuntimeStatus,
    refetchInterval: 5_000,
  });

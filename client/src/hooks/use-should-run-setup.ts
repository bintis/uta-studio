import { atom, useAtom } from "jotai";
import type { ModelDownloadTarget } from "@/types/ModelDownloadTarget";

export type SetupRequest = {
  modelTarget?: ModelDownloadTarget;
};

const setupRequestAtom = atom<SetupRequest | null>(null);

export const useShouldRunSetup = () => {
  const [setupRequest, setSetupRequest] = useAtom(setupRequestAtom);
  const setShouldRunSetup = (open: boolean) => setSetupRequest(open ? {} : null);
  return {
    shouldRunSetup: setupRequest !== null,
    setupRequest,
    openSetup: (request: SetupRequest = {}) => setSetupRequest(request),
    setShouldRunSetup,
  };
};

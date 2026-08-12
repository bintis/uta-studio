import { useQuery } from "@tanstack/react-query";
import { MENU } from "./keys";
import { loadLibraryMenuItems } from "@/bridge/library";

export const useLibraryMenuItems = () => {
  return useQuery({
    queryKey: MENU,
    queryFn: loadLibraryMenuItems,
  });
};

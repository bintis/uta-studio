import { NavInputContext, type NavAction } from "@/contexts/nav-input-context";
import { useContext, useEffect, useRef } from "react";

export type { NavAction };

export function useNavInput(handler: (action: NavAction) => void) {
  const ctx = useContext(NavInputContext);
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    if (!ctx) return;
    return ctx.subscribe((action) => handlerRef.current(action));
  }, [ctx]);
}

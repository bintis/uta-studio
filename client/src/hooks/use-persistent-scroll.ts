import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import { useMenuFocus } from "@/contexts/menu-focus-context";

export type PersistentScrollSlot = "songList" | "sidebar";

export function usePersistentScroll(slot: PersistentScrollSlot) {
  const ctx = useMenuFocus();
  const scrollRef = slot === "sidebar" ? ctx.sidebarScrollRef : ctx.scrollRef;
  const scrollTopRef = slot === "sidebar" ? ctx.sidebarScrollTopRef : ctx.scrollTopRef;
  const detachRef = useRef<(() => void) | null>(null);

  const setScrollContainer = useCallback(
    (el: HTMLElement | null) => {
      detachRef.current?.();
      detachRef.current = null;
      scrollRef.current = el;

      if (!el) {
        return;
      }

      const onScroll = () => {
        scrollTopRef.current = el.scrollTop;
      };

      el.addEventListener("scroll", onScroll, { passive: true });
      detachRef.current = () => el.removeEventListener("scroll", onScroll);
    },
    [scrollRef, scrollTopRef],
  );

  // Restore on mount after layout so scrollHeight reflects the rendered list.
  // Done in a layout effect rather than inside the ref callback because the
  // callback can fire before children's final layout in some commit orderings.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    const target = scrollTopRef.current;
    if (!el || target <= 0) {
      return;
    }
    el.scrollTop = target;
    // Belt-and-suspenders: if layout shifts after this tick (e.g. async measure),
    // re-apply once more on the next frame.
    const raf = requestAnimationFrame(() => {
      if (scrollRef.current && Math.abs(scrollRef.current.scrollTop - target) > 1) {
        scrollRef.current.scrollTop = target;
      }
    });
    return () => cancelAnimationFrame(raf);
  }, [scrollRef, scrollTopRef]);

  const resetScroll = useCallback(() => {
    scrollRef.current?.scrollTo({ top: 0 });
    scrollTopRef.current = 0;
  }, [scrollRef, scrollTopRef]);

  useEffect(() => () => detachRef.current?.(), []);

  return { setScrollContainer, resetScroll };
}

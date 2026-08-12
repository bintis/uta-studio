import { DIALOG_FOCUSABLE_SELECTOR, useDialogNav } from "@/hooks/navigation/use-dialog-nav";
import { cn } from "@/lib/utils";
import type { RefObject } from "react";
import { useMemo } from "react";
import {
  NAV,
  SETTINGS_TABS,
  VOCAL_THRESHOLD_MAX,
  VOCAL_THRESHOLD_MIN,
  VOCAL_THRESHOLD_STEP,
  getSettingsStops,
  type SettingsTab,
} from "@/components/menu/settings/constants";

const FOCUS_RING = "relative z-10 bg-foreground/[0.035] text-foreground/74";
const NO_FOCUS_RING = "focus-visible:ring-0 focus-visible:border-transparent";

function getVisibleFocusables(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>(DIALOG_FOCUSABLE_SELECTOR)).filter(
    (el) => el.offsetWidth > 0 || el.offsetHeight > 0,
  );
}

function segmentSlotFromFlatIndex(segmentSizes: readonly number[], flatIndex: number) {
  let cursor = 0;

  for (let segment = 0; segment < segmentSizes.length; segment++) {
    const size = segmentSizes[segment] ?? 0;
    if (flatIndex < cursor + size) {
      return { segment, slot: flatIndex - cursor };
    }
    cursor += size;
  }

  return null;
}

interface UseSettingsNavigationOptions {
  containerRef: RefObject<HTMLDivElement | null>;
  tab: SettingsTab;
  isParakeet: boolean;
  vocalThresholdPct: number;
  folderCount?: number;
  modelCount?: number;
  onBack: () => void;
  onTabChange: (tab: SettingsTab) => void;
  onVocalThresholdChange: (pct: number) => void;
}

export function useSettingsNavigation({
  containerRef,
  tab,
  isParakeet,
  vocalThresholdPct,
  folderCount = 0,
  modelCount = 0,
  onBack,
  onTabChange,
  onVocalThresholdChange,
}: UseSettingsNavigationOptions) {
  const stops = useMemo(
    () => getSettingsStops(tab, isParakeet, folderCount, modelCount),
    [tab, isParakeet, folderCount, modelCount],
  );
  const itemCount = useMemo(() => stops.reduce((sum, size) => sum + size, 0), [stops]);

  const { isFocused, focusSegment } = useDialogNav({
    open: true,
    itemCount,
    stops,
    onBack,
    containerRef,
    onAction: (segment, slot, action) => {
      if (segment === NAV.tabSegment && action.confirm) {
        onTabChange(SETTINGS_TABS[slot]?.value ?? "general");
        return true;
      }

      if (!action.left && !action.right) return false;

      if (tab === "analysis" && segment === NAV.analysis.vocalThreshold) {
        const delta = action.right ? VOCAL_THRESHOLD_STEP : -VOCAL_THRESHOLD_STEP;
        const next = Math.min(
          VOCAL_THRESHOLD_MAX,
          Math.max(VOCAL_THRESHOLD_MIN, vocalThresholdPct + delta),
        );
        onVocalThresholdChange(next);
        return true;
      }

      return false;
    },
  });

  const getFocusClassName = (segment: number, slot = 0) => {
    return cn(NO_FOCUS_RING, isFocused(segment, slot) && FOCUS_RING);
  };

  const syncFocusFromElement = (target: EventTarget | null) => {
    if (!containerRef.current || !(target instanceof Element)) {
      return;
    }

    const focusable = target.closest<HTMLElement>(DIALOG_FOCUSABLE_SELECTOR);
    if (!focusable || !containerRef.current.contains(focusable)) {
      return;
    }

    const flatIndex = getVisibleFocusables(containerRef.current).indexOf(focusable);
    if (flatIndex < 0) {
      return;
    }

    const next = segmentSlotFromFlatIndex(stops, flatIndex);
    if (next) {
      focusSegment(next.segment, next.slot);
    }
  };

  return {
    getFocusClassName,
    syncFocusFromElement,
  };
}

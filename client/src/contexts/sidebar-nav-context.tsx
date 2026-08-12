/**
 * Holds the flattened list of sidebar nav rows (collapses + items) and the
 * derived index maps so individual sections can resolve their focus state via
 * `useSidebarRowFocus` instead of receiving five chrome props.
 */

import { useMenuFocus } from "@/contexts/menu-focus-context";
import type { LibraryMenuSection } from "@/lib/library-menu-filter";
import { createContext, useContext, useMemo, type ReactNode } from "react";

export type SidebarNavRow =
  | { kind: "collapse"; section: LibraryMenuSection }
  | { kind: "item"; section: LibraryMenuSection; value: string }
  | { kind: "route"; id: "folders" };

interface SidebarNavContextValue {
  collapseIndexBySection: Map<LibraryMenuSection, number>;
  itemIndexBySection: Map<LibraryMenuSection, Map<string, number>>;
  routeIndexById: Map<string, number>;
}

const SidebarNavContext = createContext<SidebarNavContextValue | null>(null);

interface SidebarNavProviderProps {
  rows: SidebarNavRow[];
  baseIndex?: number;
  children: ReactNode;
}

export function SidebarNavProvider({ rows, baseIndex = 0, children }: SidebarNavProviderProps) {
  const value = useMemo<SidebarNavContextValue>(() => {
    const collapseIndexBySection = new Map<LibraryMenuSection, number>();
    const itemIndexBySection = new Map<LibraryMenuSection, Map<string, number>>();
    const routeIndexById = new Map<string, number>();

    rows.forEach((row, localIndex) => {
      const index = localIndex + baseIndex;
      if (row.kind === "collapse") {
        collapseIndexBySection.set(row.section, index);
        return;
      }
      if (row.kind === "route") {
        routeIndexById.set(row.id, index);
        return;
      }

      const sectionMap = itemIndexBySection.get(row.section) ?? new Map<string, number>();
      sectionMap.set(row.value, index);
      itemIndexBySection.set(row.section, sectionMap);
    });

    return { collapseIndexBySection, itemIndexBySection, routeIndexById };
  }, [rows, baseIndex]);

  return <SidebarNavContext.Provider value={value}>{children}</SidebarNavContext.Provider>;
}

export function useSidebarRouteFocus(id: "folders") {
  const ctx = useContext(SidebarNavContext);
  if (!ctx) throw new Error("useSidebarRouteFocus must be used within a SidebarNavProvider");
  const { focus } = useMenuFocus();
  const itemIndex = ctx.routeIndexById.get(id);
  return {
    itemIndex,
    isFocused:
      itemIndex !== undefined &&
      focus.active &&
      focus.panel === "sidebar" &&
      focus.sidebarIndex === itemIndex,
  };
}

export interface SidebarRowFocus {
  isSidebarActive: boolean;
  isCollapseFocused: boolean;
  isItemFocused: boolean;
  collapseIndex: number | undefined;
  itemIndex: number | undefined;
}

/**
 * Resolves the focus state for a sidebar nav row. Pass `value` to query a
 * leaf item; omit it to query the section's collapse trigger.
 */
export function useSidebarRowFocus(section: LibraryMenuSection, value?: string): SidebarRowFocus {
  const ctx = useContext(SidebarNavContext);
  if (!ctx) {
    throw new Error("useSidebarRowFocus must be used within a SidebarNavProvider");
  }
  const { focus } = useMenuFocus();

  const collapseIndex = ctx.collapseIndexBySection.get(section);
  const itemIndex =
    value !== undefined ? ctx.itemIndexBySection.get(section)?.get(value) : undefined;

  const isSidebarActive = focus.active && focus.panel === "sidebar";

  return {
    isSidebarActive,
    isCollapseFocused: collapseIndex !== undefined && collapseIndex === focus.sidebarIndex,
    isItemFocused: itemIndex !== undefined && itemIndex === focus.sidebarIndex,
    collapseIndex,
    itemIndex,
  };
}

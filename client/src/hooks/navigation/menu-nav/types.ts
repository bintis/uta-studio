import type { MenuFocusContextValue } from "@/contexts/menu-focus-context";
import type { RefObject } from "react";

export interface UseMenuNavOptions {
  overlayOpen: boolean;
  onBack: () => void;
}

export interface MenuNavRefs {
  onBackRef: RefObject<() => void>;
  overlayOpenRef: RefObject<boolean>;
  lastConfirmAtRef: RefObject<number>;
}

export interface MenuNavLock {
  isLocked: () => boolean;
  lockTemporarily: () => void;
}

export interface MenuNavHookOptions {
  menuFocus: MenuFocusContextValue;
  refs: MenuNavRefs;
  lock: MenuNavLock;
}

import { useMemo, useRef } from "react";
import type { MenuNavRefs, UseMenuNavOptions } from "./types";

export function useMenuNavRefs({ overlayOpen, onBack }: UseMenuNavOptions): MenuNavRefs {
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;

  const overlayOpenRef = useRef(overlayOpen);
  overlayOpenRef.current = overlayOpen;

  const lastConfirmAtRef = useRef(0);

  return useMemo(
    () => ({
      onBackRef,
      overlayOpenRef,
      lastConfirmAtRef,
    }),
    [],
  );
}

import { useEffect, useRef } from "react";

/**
 * Stack-aware "Escape closes the topmost modal" behavior.
 *
 * Every open modal/dialog/sheet/menu registers itself here. A single global
 * keydown listener (capture phase) routes one Escape press to the most recently
 * opened entry only — so stacked overlays close one layer at a time instead of
 * all collapsing at once. Propagation is stopped so app-level shortcuts don't
 * also react to the same press.
 */

type Entry = { close: () => void };

const stack: Entry[] = [];
let listening = false;

function onKeyDown(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  const top = stack[stack.length - 1];
  if (!top) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  top.close();
}

function ensureListener() {
  if (listening) return;
  // Capture phase so we run before bubble-phase app shortcut listeners.
  window.addEventListener("keydown", onKeyDown, true);
  listening = true;
}

/** When `open`, pressing Escape calls `onClose` — but only if this is the topmost modal. */
export function useModalEsc(open: boolean, onClose: () => void) {
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    const entry: Entry = { close: () => closeRef.current() };
    stack.push(entry);
    ensureListener();
    return () => {
      const i = stack.lastIndexOf(entry);
      if (i !== -1) stack.splice(i, 1);
    };
  }, [open]);
}

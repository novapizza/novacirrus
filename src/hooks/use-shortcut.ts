import { useEffect } from "react";

export type ModKey = "mod" | "shift" | "alt"; // "mod" = ⌘ on mac, Ctrl elsewhere
export type Shortcut = {
  key: string; // lowercase, e.g. "n", "arrowup", "escape", "/"
  mods?: ModKey[];
  preventDefault?: boolean;
  /** Allow the shortcut to fire even when an <input>/<textarea> is focused. */
  allowInInput?: boolean;
};

export function isMod(e: KeyboardEvent) {
  return navigator.platform.toLowerCase().includes("mac") ? e.metaKey : e.ctrlKey;
}

function matches(e: KeyboardEvent, s: Shortcut) {
  const wantMod = s.mods?.includes("mod") ?? false;
  const wantShift = s.mods?.includes("shift") ?? false;
  const wantAlt = s.mods?.includes("alt") ?? false;
  if (wantMod !== isMod(e)) return false;
  if (wantShift !== e.shiftKey) return false;
  if (wantAlt !== e.altKey) return false;
  return e.key.toLowerCase() === s.key.toLowerCase();
}

function isEditable(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || target.isContentEditable;
}

/** Bind a single shortcut. Pass handler=null to disable. */
export function useShortcut(s: Shortcut, handler: ((e: KeyboardEvent) => void) | null) {
  useEffect(() => {
    if (!handler) return;
    const fn = (e: KeyboardEvent) => {
      if (!s.allowInInput && isEditable(e.target)) return;
      if (!matches(e, s)) return;
      if (s.preventDefault !== false) e.preventDefault();
      handler(e);
    };
    window.addEventListener("keydown", fn);
    return () => window.removeEventListener("keydown", fn);
  }, [s.key, s.mods?.join(","), s.preventDefault, s.allowInInput, handler]);
}

/** Bind many at once. */
export function useShortcuts(
  bindings: Array<{ shortcut: Shortcut; handler: ((e: KeyboardEvent) => void) | null }>,
) {
  useEffect(() => {
    const fns = bindings
      .filter((b) => b.handler)
      .map(({ shortcut: s, handler }) => {
        const fn = (e: KeyboardEvent) => {
          if (!s.allowInInput && isEditable(e.target)) return;
          if (!matches(e, s)) return;
          if (s.preventDefault !== false) e.preventDefault();
          handler!(e);
        };
        window.addEventListener("keydown", fn);
        return fn;
      });
    return () => fns.forEach((f) => window.removeEventListener("keydown", f));
  }, [bindings.length, ...bindings.map((b) => b.shortcut.key + (b.shortcut.mods?.join(",") ?? ""))]);
}

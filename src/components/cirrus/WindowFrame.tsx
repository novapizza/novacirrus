import type { ReactNode } from "react";

/**
 * WindowFrame — fills the entire OS window. Tauri provides the chrome
 * (frameless + transparent + vibrancy applied from Rust). Children own
 * the internal layout (typically a resizable panel group).
 *
 * Rounded corners + border are CSS, visible because the window is
 * transparent with no native decorations.
 */
export function WindowFrame({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden rounded-2xl border border-border/60 bg-background/70">
      {children}
    </div>
  );
}

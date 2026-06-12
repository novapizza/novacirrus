import { useEffect, useRef, type ReactNode } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useModalEsc } from "@/hooks/use-modal-esc";

export type MenuItem =
  | {
      type: "item";
      label: string;
      icon?: ReactNode;
      onClick: () => void;
      danger?: boolean;
      disabled?: boolean;
      shortcut?: string;
    }
  | { type: "separator" };

type Props = {
  open: boolean;
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
};

export function ContextMenu({ open, x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement | null>(null);

  useModalEsc(open, onClose);

  useEffect(() => {
    if (!open) return;
    const onDocDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", onDocDown);
    return () => window.removeEventListener("mousedown", onDocDown);
  }, [open, onClose]);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          ref={ref}
          initial={{ opacity: 0, scale: 0.96, y: -4 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.96, y: -4, transition: { duration: 0.1 } }}
          transition={{ type: "spring", stiffness: 460, damping: 32 }}
          className="popover-shadow vibrancy fixed z-[80] min-w-[220px] overflow-hidden rounded-lg border border-border/60 bg-popover/95 p-1"
          style={{ left: x, top: y }}
        >
          {items.map((it, i) =>
            it.type === "separator" ? (
              <div key={i} className="my-1 h-px bg-border/60" />
            ) : (
              <button
                key={i}
                disabled={it.disabled}
                onClick={() => {
                  it.onClick();
                  onClose();
                }}
                className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-[12.5px] disabled:cursor-not-allowed disabled:opacity-40 ${
                  it.danger
                    ? "text-destructive hover:bg-destructive/10"
                    : "text-foreground hover:bg-accent"
                }`}
              >
                <span className="flex h-4 w-4 items-center justify-center text-muted-foreground">
                  {it.icon}
                </span>
                <span className="flex-1 truncate">{it.label}</span>
                {it.shortcut && (
                  <span className="font-mono text-[10.5px] tracking-wider text-muted-foreground/80">
                    {it.shortcut}
                  </span>
                )}
              </button>
            ),
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

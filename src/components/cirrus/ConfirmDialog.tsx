import { motion, AnimatePresence } from "framer-motion";
import { AlertTriangle } from "lucide-react";
import { useEffect } from "react";
import { useModalEsc } from "@/hooks/use-modal-esc";
import { useFocusTrap } from "@/hooks/use-focus-trap";

type Props = {
  open: boolean;
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
};

export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  danger,
  onConfirm,
  onCancel,
}: Props) {
  useModalEsc(open, onCancel);
  const trapRef = useFocusTrap<HTMLDivElement>(open);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        onConfirm();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onConfirm]);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          className="fixed inset-0 z-[90] flex items-center justify-center"
          onClick={onCancel}
        >
          <div className="absolute inset-0 bg-black/30 backdrop-blur-sm" />

          <motion.div
            ref={trapRef}
            role="alertdialog"
            aria-modal="true"
            aria-label={title}
            tabIndex={-1}
            onClick={(e) => e.stopPropagation()}
            initial={{ y: -8, scale: 0.97, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1 }}
            exit={{ y: -8, scale: 0.97, opacity: 0 }}
            transition={{ type: "spring", stiffness: 420, damping: 30 }}
            className="popover-shadow vibrancy relative w-[400px] overflow-hidden rounded-xl border border-border/60 bg-popover/95 outline-none"
          >
            <div className="flex items-start gap-3 p-5">
              {danger && (
                <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-destructive/15 text-destructive">
                  <AlertTriangle className="h-4 w-4" />
                </div>
              )}
              <div className="min-w-0 flex-1">
                <h3 className="font-display text-[14px] font-semibold tracking-tight">{title}</h3>
                {message && (
                  <p className="mt-1 text-[12px] leading-relaxed text-muted-foreground">{message}</p>
                )}
              </div>
            </div>
            <div className="flex items-center justify-end gap-2 border-t border-border/60 bg-background/30 px-5 py-3">
              <button
                onClick={onCancel}
                className="h-8 rounded-md border border-border/60 bg-card/60 px-3 text-[12px] font-medium text-foreground/80 hover:bg-foreground/5"
              >
                {cancelLabel}
              </button>
              <button
                autoFocus
                onClick={onConfirm}
                className={`h-8 rounded-md px-3 text-[12px] font-semibold shadow-sm transition hover:brightness-110 active:scale-[0.98] ${
                  danger
                    ? "bg-destructive text-destructive-foreground"
                    : "bg-primary text-primary-foreground"
                }`}
              >
                {confirmLabel}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

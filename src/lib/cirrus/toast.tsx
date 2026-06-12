import { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { CheckCircle2, AlertCircle, Info, X } from "lucide-react";

export type ToastKind = "success" | "error" | "info";
export type Toast = {
  id: string;
  kind: ToastKind;
  title: string;
  message?: string;
  duration?: number;
};

let _toasts: Toast[] = [];
let _subs = new Set<(t: Toast[]) => void>();

function notify() {
  for (const s of _subs) s(_toasts);
}

export const toast = {
  show(t: Omit<Toast, "id">) {
    const id = Math.random().toString(36).slice(2);
    const full: Toast = { id, duration: 4000, ...t };
    _toasts = [..._toasts, full];
    notify();
    if (full.duration && full.duration > 0) {
      setTimeout(() => toast.dismiss(id), full.duration);
    }
    return id;
  },
  success(title: string, message?: string) {
    return toast.show({ kind: "success", title, message });
  },
  error(title: string, message?: string) {
    return toast.show({ kind: "error", title, message, duration: 6000 });
  },
  info(title: string, message?: string) {
    return toast.show({ kind: "info", title, message });
  },
  dismiss(id: string) {
    _toasts = _toasts.filter((t) => t.id !== id);
    notify();
  },
  dismissAll() {
    _toasts = [];
    notify();
  },
};

export function ToastViewport() {
  const [items, setItems] = useState<Toast[]>(_toasts);
  useEffect(() => {
    _subs.add(setItems);
    return () => {
      _subs.delete(setItems);
    };
  }, []);

  return (
    <div className="pointer-events-none fixed right-4 top-14 z-[100] flex w-[340px] flex-col gap-2">
      <AnimatePresence initial={false}>
        {items.map((t) => (
          <motion.div
            key={t.id}
            layout
            initial={{ opacity: 0, y: -8, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, x: 16, transition: { duration: 0.15 } }}
            transition={{ type: "spring", stiffness: 380, damping: 30 }}
            className="popover-shadow pointer-events-auto flex gap-3 rounded-xl border border-border/60 bg-popover/95 p-3 backdrop-blur"
          >
            <Icon kind={t.kind} />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[13px] font-medium text-foreground">{t.title}</div>
              {t.message && (
                <div className="mt-0.5 line-clamp-3 text-[11.5px] text-muted-foreground">
                  {t.message}
                </div>
              )}
            </div>
            <button
              onClick={() => toast.dismiss(t.id)}
              className="-mr-1 flex h-5 w-5 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-foreground/5 hover:text-foreground"
            >
              <X className="h-3 w-3" />
            </button>
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}

function Icon({ kind }: { kind: ToastKind }) {
  const cn = "h-4 w-4 mt-0.5 shrink-0";
  if (kind === "success") return <CheckCircle2 className={cn + " text-success"} />;
  if (kind === "error") return <AlertCircle className={cn + " text-destructive"} />;
  return <Info className={cn + " text-primary"} />;
}

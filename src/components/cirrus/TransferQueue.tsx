import { memo, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { ArrowUp, ArrowDown, X, ChevronUp, ChevronDown, Check, AlertCircle } from "lucide-react";
import { formatBytes } from "@/lib/cirrus/types";
import { useTransfers, clearDoneTransfers, dismissTransfer } from "@/lib/cirrus/transfers";
import type { Transfer } from "@/lib/cirrus/types";

export function TransferQueue() {
  const transfers = useTransfers();
  const [open, setOpen] = useState(true);
  const active = transfers.filter((t) => t.status === "active");
  const done = transfers.filter((t) => t.status === "done").length;
  const failed = transfers.filter((t) => t.status === "error").length;
  const totalProgress = aggregate(active);

  return (
    <div className="titlebar-bg border-t border-border/60">
      <div className="flex w-full items-center gap-3 px-3 py-1.5">
        <button
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          aria-label={open ? "Collapse transfer list" : "Expand transfer list"}
          className="flex items-center gap-1.5 rounded text-[11px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground"
        >
          {open ? <ChevronDown className="h-3 w-3" /> : <ChevronUp className="h-3 w-3" />}
          Transfers
        </button>
        <span className="rounded bg-foreground/5 px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
          {active.length} active · {done} done
          {failed > 0 && <span className="text-destructive"> · {failed} failed</span>}
        </span>

        <div className="ml-3 flex flex-1 items-center gap-2">
          <div
            role="progressbar"
            aria-label="Overall transfer progress"
            aria-valuenow={Math.round(totalProgress * 100)}
            aria-valuemin={0}
            aria-valuemax={100}
            className="relative h-1 flex-1 overflow-hidden rounded-full bg-foreground/10"
          >
            <motion.div
              className="absolute inset-y-0 left-0 bg-gradient-to-r from-primary to-[oklch(0.7_0.18_220)]"
              animate={{ width: `${totalProgress * 100}%` }}
              transition={{ type: "spring", stiffness: 80, damping: 20 }}
            />
          </div>
          <span className="font-mono text-[11px] text-muted-foreground tabular-nums">
            {Math.round(totalProgress * 100)}%
          </span>
        </div>

        {done + failed > 0 && (
          <button
            onClick={clearDoneTransfers}
            className="rounded px-1.5 py-0.5 text-[10px] text-muted-foreground hover:bg-foreground/5 hover:text-foreground"
          >
            Clear finished
          </button>
        )}
      </div>

      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2, ease: [0.4, 0, 0.2, 1] }}
            className="overflow-hidden border-t border-border/40"
          >
            <div className="max-h-44 overflow-y-auto scrollbar-mac">
              {transfers.length === 0 && (
                <div className="px-3 py-4 text-center text-[12px] text-muted-foreground">
                  No transfers yet.
                </div>
              )}
              {transfers.map((t) => (
                <TransferRow key={t.id} t={t} />
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function aggregate(active: Transfer[]) {
  if (active.length === 0) return 1;
  const totals = active.reduce((s, t) => s + (t.total || 0), 0);
  const transferred = active.reduce((s, t) => s + (t.transferred || 0), 0);
  if (totals === 0) return 0;
  return Math.min(1, transferred / totals);
}

const TransferRow = memo(function TransferRow({ t }: { t: Transfer }) {
  const isDone = t.status === "done";
  const isError = t.status === "error";
  const progress = t.total > 0 ? t.transferred / t.total : 0;

  return (
    <div className="grid grid-cols-[24px_1fr_180px_90px_60px] items-center gap-3 border-b border-border/30 px-3 py-2 text-[12px] last:border-b-0 hover:bg-foreground/[0.03]">
      <div
        className={`flex h-5 w-5 items-center justify-center rounded-full ${
          isDone
            ? "bg-success/20 text-success"
            : isError
              ? "bg-destructive/20 text-destructive"
              : "bg-primary/15 text-primary"
        }`}
      >
        {isDone ? (
          <Check className="h-3 w-3" />
        ) : isError ? (
          <AlertCircle className="h-3 w-3" />
        ) : t.direction === "up" ? (
          <ArrowUp className="h-3 w-3" />
        ) : (
          <ArrowDown className="h-3 w-3" />
        )}
      </div>

      <div className="min-w-0">
        <div className="truncate font-medium text-foreground">{t.name}</div>
        <div
          className={`truncate font-mono text-[10.5px] ${isError ? "text-destructive" : "text-muted-foreground"}`}
          title={isError && t.error?.detail ? t.error.detail : undefined}
        >
          {isError
            ? t.error
              ? t.error.remediation
                ? `${t.error.summary} — ${t.error.remediation}`
                : t.error.summary
              : "Transfer failed"
            : `${formatBytes(t.transferred)} / ${formatBytes(t.total)}`}
        </div>
      </div>

      <div className="flex items-center gap-2">
        <div
          role="progressbar"
          aria-label={`${t.name} progress`}
          aria-valuenow={Math.round(progress * 100)}
          aria-valuemin={0}
          aria-valuemax={100}
          className="relative h-1 flex-1 overflow-hidden rounded-full bg-foreground/10"
        >
          <div
            className={`absolute inset-y-0 left-0 ${
              isDone ? "bg-success" : isError ? "bg-destructive" : "bg-primary"
            }`}
            style={{ width: `${progress * 100}%` }}
          />
        </div>
        <span className="font-mono text-[11px] tabular-nums text-muted-foreground">
          {Math.round(progress * 100)}%
        </span>
      </div>

      <div className="font-mono text-[11px] tabular-nums text-muted-foreground">
        {isDone ? formatBytes(t.total) : isError ? "Error" : t.direction === "up" ? "↑" : "↓"}
      </div>

      <div className="flex items-center justify-end gap-0.5">
        {(isDone || isError) && (
          <button
            onClick={() => dismissTransfer(t.id)}
            aria-label={`Remove ${t.name} from list`}
            title="Remove from list"
            className="flex h-6 w-6 items-center justify-center rounded text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
          >
            <X className="h-3 w-3" />
          </button>
        )}
      </div>
    </div>
  );
});

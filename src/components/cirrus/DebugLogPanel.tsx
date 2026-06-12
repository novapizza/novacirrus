import { useMemo, useState } from "react";
import { Bug, Trash2, ChevronDown } from "lucide-react";
import { useLogs, clearLogs, LEVEL_RANK } from "@/lib/cirrus/logs";
import type { LogEntry, LogLevel, StatusCode } from "@/lib/cirrus/types";

const LEVELS: LogLevel[] = ["debug", "info", "warn", "error"];

const LEVEL_STYLE: Record<LogLevel, string> = {
  error: "text-destructive",
  warn: "text-warning",
  info: "text-primary",
  debug: "text-muted-foreground",
};

const LEVEL_TAG: Record<LogLevel, string> = {
  error: "ERR",
  warn: "WRN",
  info: "INF",
  debug: "DBG",
};

const ALL_PHASES = "all";

function codeLabel(c: StatusCode): string {
  const proto = c.proto === "http" ? "HTTP" : c.proto.toUpperCase();
  return `${proto} ${c.value}`;
}

export function DebugLogPanel() {
  const logs = useLogs();
  const [enabled, setEnabled] = useState(false);
  const [minLevel, setMinLevel] = useState<LogLevel>("info");
  const [phase, setPhase] = useState<string>(ALL_PHASES);

  // Phases actually present in the buffer — the filter is self-populating, so a
  // newly-added Phase (e.g. tls / passive) shows up here with no UI change.
  const phases = useMemo(() => {
    const set = new Set<string>();
    for (const l of logs) if (l.phase) set.add(l.phase);
    return Array.from(set).sort();
  }, [logs]);

  const filtered = useMemo(
    () =>
      logs
        .filter((l) => LEVEL_RANK[l.level] >= LEVEL_RANK[minLevel])
        .filter((l) => phase === ALL_PHASES || l.phase === phase)
        .slice()
        .reverse(), // newest first
    [logs, minLevel, phase],
  );

  if (!enabled) {
    return (
      <div className="border-t border-sidebar-border px-3 py-2">
        <button
          onClick={() => setEnabled(true)}
          className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-[12px] text-muted-foreground transition hover:bg-foreground/5 hover:text-foreground"
        >
          <Bug className="h-3.5 w-3.5" />
          Debug Log
          <span className="ml-auto rounded bg-foreground/5 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wider">
            Off
          </span>
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-[38%] min-h-[150px] flex-col border-t border-sidebar-border">
      <div className="flex items-center gap-2 px-3 py-1.5">
        <Bug className="h-3.5 w-3.5 text-muted-foreground" />
        <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          Debug Log
        </span>
        <select
          value={phase}
          onChange={(e) => setPhase(e.target.value)}
          title="Filter by phase"
          className="ml-auto rounded border border-border/60 bg-card/60 px-1 py-0.5 text-[10px] text-foreground focus:outline-none"
        >
          <option value={ALL_PHASES}>All phases</option>
          {phases.map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
        <select
          value={minLevel}
          onChange={(e) => setMinLevel(e.target.value as LogLevel)}
          title="Minimum severity"
          className="rounded border border-border/60 bg-card/60 px-1 py-0.5 text-[10px] text-foreground focus:outline-none"
        >
          {LEVELS.map((l) => (
            <option key={l} value={l}>
              {l.toUpperCase()}+
            </option>
          ))}
        </select>
        <button
          onClick={clearLogs}
          title="Clear log"
          className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground hover:bg-foreground/10 hover:text-foreground"
        >
          <Trash2 className="h-3 w-3" />
        </button>
        <button
          onClick={() => setEnabled(false)}
          title="Hide debug log"
          className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground hover:bg-foreground/10 hover:text-foreground"
        >
          <ChevronDown className="h-3.5 w-3.5" />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto scrollbar-mac px-2 pb-2 font-mono text-[10.5px] leading-relaxed">
        {filtered.length === 0 ? (
          <div className="px-1 py-3 text-center text-[11px] text-muted-foreground">
            No log entries at this level yet.
          </div>
        ) : (
          filtered.map((l) => <LogRow key={l.id} l={l} />)
        )}
      </div>
    </div>
  );
}

function LogRow({ l }: { l: LogEntry }) {
  const tooltip = [
    new Date(l.ts).toLocaleString(),
    l.scope,
    l.connection,
    l.connector,
    l.phase && `phase=${l.phase}`,
    l.category && `category=${l.category}`,
    l.code && codeLabel(l.code),
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div
      title={`${tooltip}\n${l.message}`}
      className="flex items-start gap-1.5 rounded px-1 py-0.5 hover:bg-foreground/[0.04]"
    >
      <span className="shrink-0 tabular-nums text-muted-foreground/70">
        {new Date(l.ts).toLocaleTimeString(undefined, { hour12: false })}
      </span>
      <span className={`shrink-0 font-semibold ${LEVEL_STYLE[l.level]}`}>{LEVEL_TAG[l.level]}</span>
      {l.phase && (
        <span className="shrink-0 rounded bg-foreground/[0.06] px-1 text-[9.5px] uppercase tracking-wide text-muted-foreground/80">
          {l.phase}
        </span>
      )}
      {l.code && (
        <span className="shrink-0 rounded bg-destructive/10 px-1 text-[9.5px] tabular-nums text-destructive/90">
          {codeLabel(l.code)}
        </span>
      )}
      <span className="min-w-0 break-words text-foreground/80">
        {l.connection && <span className="text-muted-foreground/70">[{l.connection}] </span>}
        {l.message}
      </span>
    </div>
  );
}

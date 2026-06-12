import { motion } from "framer-motion";
import { Server, Cloud, Plus, Settings, AlertCircle, RefreshCw } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { type Connection, KIND_LABEL, isS3Family } from "@/lib/cirrus/types";
import { DebugLogPanel } from "./DebugLogPanel";

type Props = {
  connections: Connection[];
  activeId: string | null;
  onSelect: (c: Connection) => void;
  onNewConnection: () => void;
  loading?: boolean;
  error?: string | null;
  onRetry?: () => void;
};

export function Sidebar({ connections, activeId, onSelect, onNewConnection, loading, error, onRetry }: Props) {
  const cloud = connections.filter((c) => isS3Family(c.kind));
  const servers = connections.filter((c) => !isS3Family(c.kind));

  return (
    <aside className="sidebar-bg flex h-full w-full flex-col">
      <div className="px-3 pt-3 pb-2">
        <SectionLabel>Quick Access</SectionLabel>
        <Link
          to="/connections"
          className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-[13px] text-sidebar-foreground transition hover:bg-foreground/5"
        >
          <span className="text-muted-foreground">
            <Settings className="h-3.5 w-3.5" />
          </span>
          Manage connections
        </Link>
      </div>

      <div className="flex-1 overflow-y-auto scrollbar-mac px-3 pb-3">
        {servers.length > 0 && (
          <Group title="Servers" connections={servers} activeId={activeId} onSelect={onSelect} />
        )}
        {cloud.length > 0 && (
          <Group title="Cloud" connections={cloud} activeId={activeId} onSelect={onSelect} />
        )}
        {error && (
          <div className="mt-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-3 text-[12px] text-destructive">
            <div className="flex items-center gap-1.5 font-medium">
              <AlertCircle className="h-3.5 w-3.5 shrink-0" />
              Couldn’t load connections
            </div>
            <div className="mt-1 break-words text-[11px] text-destructive/80">{error}</div>
            {onRetry && (
              <button
                onClick={onRetry}
                className="mt-2 flex items-center gap-1 rounded border border-destructive/30 px-1.5 py-0.5 text-[11px] hover:bg-destructive/10"
              >
                <RefreshCw className="h-3 w-3" />
                Retry
              </button>
            )}
          </div>
        )}
        {!error && loading && connections.length === 0 && (
          <div className="mt-3 px-3 py-4 text-center text-[12px] text-muted-foreground">
            Loading connections…
          </div>
        )}
        {!error && !loading && connections.length === 0 && (
          <div className="mt-3 rounded-md border border-dashed border-sidebar-border px-3 py-4 text-center text-[12px] text-muted-foreground">
            No connections yet.
          </div>
        )}

        <button
          onClick={onNewConnection}
          className="mt-4 flex w-full items-center gap-2 rounded-md border border-dashed border-sidebar-border px-2 py-1.5 text-[12px] text-muted-foreground transition hover:border-primary/50 hover:text-foreground"
        >
          <Plus className="h-3.5 w-3.5" />
          Add connection
        </button>
      </div>

      <DebugLogPanel />

      <div className="border-t border-sidebar-border px-3 py-2 text-[11px] text-muted-foreground">
        v0.1 · alpha
      </div>
    </aside>
  );
}

function Group({
  title,
  connections,
  activeId,
  onSelect,
}: {
  title: string;
  connections: Connection[];
  activeId: string | null;
  onSelect: (c: Connection) => void;
}) {
  return (
    <div className="mt-3">
      <SectionLabel>{title}</SectionLabel>
      {connections.map((s) => {
        const active = s.id === activeId;
        const Icon = isS3Family(s.kind) ? Cloud : Server;
        return (
          <button
            key={s.id}
            onClick={() => onSelect(s)}
            className={`group relative flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition ${
              active
                ? "bg-sidebar-accent text-sidebar-accent-foreground"
                : "text-sidebar-foreground hover:bg-foreground/5"
            }`}
          >
            {active && (
              <motion.span
                layoutId="sidebar-active"
                className="absolute inset-0 -z-0 rounded-md bg-sidebar-accent"
                transition={{ type: "spring", stiffness: 500, damping: 40 }}
              />
            )}
            <span className="relative z-10 flex w-full items-center gap-2">
              <span
                className="h-2 w-2 rounded-full ring-2 ring-background/40"
                style={{ backgroundColor: s.color }}
              />
              <Icon className="h-3.5 w-3.5 opacity-70" />
              <span className="truncate font-medium">{s.name}</span>
              <span
                className={`ml-auto rounded px-1 py-0.5 text-[9px] font-mono uppercase tracking-wider ${
                  active ? "bg-white/20 text-current" : "bg-foreground/5 text-muted-foreground"
                }`}
              >
                {KIND_LABEL[s.kind]}
              </span>
            </span>
          </button>
        );
      })}
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-2 pb-1 text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground/80">
      {children}
    </div>
  );
}


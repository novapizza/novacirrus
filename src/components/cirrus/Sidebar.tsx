import { motion } from "framer-motion";
import { Server, Cloud, Plus, Settings, AlertCircle, RefreshCw, Loader2 } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { type Connection, KIND_LABEL, isS3Family } from "@/lib/cirrus/types";
import { DebugLogPanel } from "./DebugLogPanel";

type Props = {
  connections: Connection[];
  /** The connection that is actually connected (drives the remote pane). */
  connectedId: string | null;
  /** The highlighted connection (single click) — does not connect. */
  selectedId: string | null;
  /** The connection whose handshake is in flight, if any. */
  connectingId: string | null;
  /** Single click: highlight only, no network. */
  onSelect: (c: Connection) => void;
  /** Double click / context "Connect": open the session. */
  onConnect: (c: Connection) => void;
  /** Right click: open the connection context menu at the given screen point. */
  onContextMenu: (c: Connection, x: number, y: number) => void;
  onNewConnection: () => void;
  loading?: boolean;
  error?: string | null;
  onRetry?: () => void;
};

export function Sidebar({
  connections,
  connectedId,
  selectedId,
  connectingId,
  onSelect,
  onConnect,
  onContextMenu,
  onNewConnection,
  loading,
  error,
  onRetry,
}: Props) {
  const cloud = connections.filter((c) => isS3Family(c.kind));
  const servers = connections.filter((c) => !isS3Family(c.kind));
  const rowProps = { connectedId, selectedId, connectingId, onSelect, onConnect, onContextMenu };

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
          <Group title="Servers" connections={servers} {...rowProps} />
        )}
        {cloud.length > 0 && (
          <Group title="Cloud" connections={cloud} {...rowProps} />
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
  connectedId,
  selectedId,
  connectingId,
  onSelect,
  onConnect,
  onContextMenu,
}: {
  title: string;
  connections: Connection[];
  connectedId: string | null;
  selectedId: string | null;
  connectingId: string | null;
  onSelect: (c: Connection) => void;
  onConnect: (c: Connection) => void;
  onContextMenu: (c: Connection, x: number, y: number) => void;
}) {
  return (
    <div className="mt-3">
      <SectionLabel>{title}</SectionLabel>
      {connections.map((s) => {
        const connected = s.id === connectedId;
        const selected = s.id === selectedId;
        const connecting = s.id === connectingId;
        const Icon = isS3Family(s.kind) ? Cloud : Server;
        return (
          <button
            key={s.id}
            onClick={() => onSelect(s)}
            onDoubleClick={() => onConnect(s)}
            onContextMenu={(e) => {
              e.preventDefault();
              onContextMenu(s, e.clientX, e.clientY);
            }}
            title={connected ? "Connected — double-click to reconnect" : "Double-click or right-click to connect"}
            className={`group relative flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition ${
              connected
                ? "bg-sidebar-accent text-sidebar-accent-foreground"
                : selected
                  ? "bg-foreground/[0.07] text-sidebar-foreground ring-1 ring-inset ring-sidebar-border"
                  : "text-sidebar-foreground hover:bg-foreground/5"
            }`}
          >
            {connected && (
              <motion.span
                layoutId="sidebar-active"
                className="absolute inset-0 -z-0 rounded-md bg-sidebar-accent"
                transition={{ type: "spring", stiffness: 500, damping: 40 }}
              />
            )}
            <span className="relative z-10 flex w-full items-center gap-2">
              <span
                className={`h-2 w-2 rounded-full ring-2 ${
                  connected ? "ring-success/70" : "ring-background/40"
                }`}
                style={{ backgroundColor: s.color }}
              />
              <Icon className="h-3.5 w-3.5 opacity-70" />
              <span className="truncate font-medium">{s.name}</span>
              {connecting ? (
                <Loader2 className="h-3 w-3 shrink-0 animate-spin text-muted-foreground" />
              ) : connected ? (
                <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-success" title="Connected" />
              ) : null}
              <span
                className={`ml-auto rounded px-1 py-0.5 text-[9px] font-mono uppercase tracking-wider ${
                  connected ? "bg-white/20 text-current" : "bg-foreground/5 text-muted-foreground"
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


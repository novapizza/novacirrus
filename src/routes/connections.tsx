import { createFileRoute, Link } from "@tanstack/react-router";
import { useState } from "react";
import { ArrowLeft, Plus, Pencil, Trash2, Plug, Server, Cloud } from "lucide-react";
import { TitleBar } from "@/components/cirrus/TitleBar";
import { WindowFrame } from "@/components/cirrus/WindowFrame";
import { Screen, ScreenHeader, ScreenBody, Section } from "@/components/cirrus/Screen";
import { ConnectSheet } from "@/components/cirrus/ConnectSheet";
import { Button } from "@/components/ui/button";
import { useConnections } from "@/hooks/use-connections";
import { type Connection, KIND_LABEL, isS3Family } from "@/lib/cirrus/types";
import { api } from "@/lib/cirrus/ipc";
import { errText } from "@/lib/cirrus/errors";

export const Route = createFileRoute("/connections")({
  component: ConnectionsScreen,
});

function ConnectionsScreen() {
  const { items, remove, refresh } = useConnections();
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState<Connection | null>(null);
  const [testing, setTesting] = useState<string | null>(null);
  const [result, setResult] = useState<Record<string, string>>({});

  const openNew = () => {
    setEditing(null);
    setOpen(true);
  };
  const openEdit = (c: Connection) => {
    setEditing(c);
    setOpen(true);
  };

  const onTest = async (id: string) => {
    setTesting(id);
    try {
      const msg = await api.testConnection(id);
      setResult((r) => ({ ...r, [id]: msg }));
    } catch (e) {
      setResult((r) => ({ ...r, [id]: errText(e) }));
    } finally {
      setTesting(null);
    }
  };

  return (
    <WindowFrame>
      <TitleBar onConnect={openNew} />
      <div className="flex flex-1 overflow-hidden">
        <Screen>
            <ScreenHeader
              title="Connections"
              subtitle="Manage saved servers and cloud buckets"
              actions={
                <>
                  <Link to="/">
                    <Button variant="ghost" size="sm" className="gap-1.5">
                      <ArrowLeft className="h-3.5 w-3.5" /> Back
                    </Button>
                  </Link>
                  <Button size="sm" onClick={openNew} className="gap-1.5">
                    <Plus className="h-3.5 w-3.5" /> New
                  </Button>
                </>
              }
            />
            <ScreenBody>
              <Section
                title={`All connections (${items.length})`}
                description="Secrets are stored in your OS keychain (Keychain on macOS, Credential Manager on Windows)."
              >
                {items.length === 0 && (
                  <div className="px-3 py-8 text-center text-[12px] text-muted-foreground">
                    No connections yet. Click <strong>New</strong> to add one.
                  </div>
                )}
                {items.map((c) => {
                  const Icon = isS3Family(c.kind) ? Cloud : Server;
                  return (
                    <div
                      key={c.id}
                      className="flex items-center gap-3 rounded-lg px-3 py-2 hover:bg-accent/60"
                    >
                      <span
                        className="h-2 w-2 rounded-full ring-2 ring-background/40"
                        style={{ backgroundColor: c.color }}
                      />
                      <Icon className="h-4 w-4 text-muted-foreground" />
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[13px] font-medium text-foreground">{c.name}</div>
                        <div className="truncate font-mono text-[11px] text-muted-foreground">
                          {detailLine(c)}
                        </div>
                        {result[c.id] && (
                          <div
                            className={`mt-0.5 truncate text-[11px] ${
                              result[c.id].startsWith("OK")
                                ? "text-success"
                                : "text-destructive"
                            }`}
                          >
                            {result[c.id]}
                          </div>
                        )}
                      </div>
                      <span className="shrink-0 rounded bg-foreground/5 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-muted-foreground">
                        {KIND_LABEL[c.kind]}
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => onTest(c.id)}
                        disabled={testing === c.id}
                        className="gap-1.5"
                      >
                        <Plug className="h-3.5 w-3.5" />
                        {testing === c.id ? "Testing…" : "Test"}
                      </Button>
                      <Button variant="ghost" size="sm" onClick={() => openEdit(c)} className="gap-1.5">
                        <Pencil className="h-3.5 w-3.5" /> Edit
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={async () => {
                          if (confirm(`Delete connection "${c.name}"?`)) {
                            await remove(c.id);
                            await refresh();
                          }
                        }}
                        className="gap-1.5 text-destructive hover:text-destructive"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  );
                })}
              </Section>
            </ScreenBody>
          </Screen>
      </div>

      <ConnectSheet
        open={open}
        initial={editing}
        onClose={() => {
          setOpen(false);
          setEditing(null);
        }}
        onSaved={async () => {
          await refresh();
          setOpen(false);
          setEditing(null);
        }}
      />
    </WindowFrame>
  );
}

function detailLine(c: Connection) {
  if (isS3Family(c.kind)) {
    const parts = [c.endpoint || "default", c.region, c.bucket ? `bucket=${c.bucket}` : null].filter(Boolean);
    return parts.join(" · ");
  }
  return `${c.username ?? ""}@${c.host ?? ""}${c.port ? `:${c.port}` : ""}`;
}

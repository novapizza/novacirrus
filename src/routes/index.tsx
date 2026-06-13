import { createFileRoute } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save as saveDialog, open as openDialog } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { listen } from "@tauri-apps/api/event";
import { PanelGroup, Panel } from "react-resizable-panels";
import { TitleBar } from "@/components/cirrus/TitleBar";
import { Sidebar } from "@/components/cirrus/Sidebar";
import { FilePane } from "@/components/cirrus/FilePane";
import { TransferQueue } from "@/components/cirrus/TransferQueue";
import { ConnectSheet } from "@/components/cirrus/ConnectSheet";
import { WindowFrame } from "@/components/cirrus/WindowFrame";
import { VHandle, HHandle } from "@/components/cirrus/Resize";
import { ConfirmDialog } from "@/components/cirrus/ConfirmDialog";
import { ContextMenu } from "@/components/cirrus/ContextMenu";
import { SettingsSheet, useSettings } from "@/components/cirrus/SettingsSheet";
import { FileUp, FolderUp, Plug, Unplug } from "lucide-react";
import type { FileNode } from "@/lib/cirrus/types";
import { useConnections } from "@/hooks/use-connections";
import { useShortcuts } from "@/hooks/use-shortcut";
import { api } from "@/lib/cirrus/ipc";
import { errText } from "@/lib/cirrus/errors";
import { toast } from "@/lib/cirrus/toast";
import {
  type Connection,
  isS3Family,
  KIND_LABEL,
  type ObjectEntry,
  type LocalEntry,
} from "@/lib/cirrus/types";

export const Route = createFileRoute("/")({
  component: Index,
});

type Side = "local" | "remote";

function Index() {
  const { items: connections, refresh, loading: connectionsLoading, error: connectionsError } = useConnections();

  // `activeId` = the connected session (drives the remote pane). `selectedId` =
  // the merely-highlighted row. Single click selects; an explicit Connect
  // (double-click / context menu) is what opens a session — so clicking around
  // the sidebar never reconnects, and switching selection never disconnects.
  const [activeId, setActiveId] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [connMenu, setConnMenu] = useState<{ c: Connection; x: number; y: number } | null>(null);
  const [connectOpen, setConnectOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [confirm, setConfirm] = useState<{
    title: string;
    message?: string;
    onConfirm: () => void;
  } | null>(null);
  const [focusedSide, setFocusedSide] = useState<Side>("local");
  const [settings, setSet] = useSettings();

  // Search box text — live-filters the focused pane by name.
  const [searchQuery, setSearchQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  // Recursive remote search results (a flat snapshot for `term`), or null when browsing.
  const [remoteSearch, setRemoteSearch] = useState<{ term: string; results: FileNode[] } | null>(null);
  const [remoteSearching, setRemoteSearching] = useState(false);
  // Position of the top-bar Upload menu (files / folder), or null when closed.
  const [uploadMenu, setUploadMenu] = useState<{ x: number; y: number } | null>(null);

  const clearSearch = useCallback(() => {
    setSearchQuery("");
    setRemoteSearch(null);
  }, []);

  // Remote: single canonical path.
  // S3-family: "bucket/key/prefix" (no leading slash). Empty = list buckets.
  // SFTP/FTP: "/absolute/path" or "" for the server's default.
  const [remotePath, setRemotePath] = useState("");
  const [remoteFiles, setRemoteFiles] = useState<FileNode[]>([]);
  const [remoteLoading, setRemoteLoading] = useState(false);

  // Local
  const [localCwd, setLocalCwd] = useState<string>("");
  const [localFiles, setLocalFiles] = useState<FileNode[]>([]);
  const [localLoading, setLocalLoading] = useState(false);

  const active = useMemo<Connection | null>(
    () => connections.find((c) => c.id === activeId) ?? null,
    [activeId, connections],
  );

  // Single click: highlight only — no network, no disconnect of the current session.
  const selectConnection = useCallback((c: Connection) => setSelectedId(c.id), []);
  // Explicit connect (double-click / context menu): open and pool the live
  // session, then show it as active. Auth/host-key/network errors surface here
  // instead of failing silently on the first list.
  const connectConnection = useCallback(async (c: Connection) => {
    setSelectedId(c.id);
    if (c.id === activeId) {
      // Reconnect: drop the stale session first so connect() opens a fresh one.
      await api.disconnect(c.id).catch(() => {});
      setActiveId(null);
    }
    setConnectingId(c.id);
    try {
      await api.connect(c.id);
      setActiveId(c.id);
    } catch (e) {
      toast.error("Couldn’t connect", errText(e));
    } finally {
      setConnectingId((id) => (id === c.id ? null : id));
    }
  }, [activeId]);
  const disconnectConnection = useCallback(async () => {
    const id = activeId;
    setActiveId(null);
    if (id) await api.disconnect(id).catch(() => {});
  }, [activeId]);

  // The server (or a network drop) can close a pooled session mid-use; the
  // backend evicts it and emits `disconnected`. Clear the active connection so
  // the UI matches reality rather than showing a dead session as connected.
  useEffect(() => {
    const un = listen<{ id: string; reason: string }>("disconnected", (e) => {
      setConnectingId((id) => (id === e.payload.id ? null : id));
      setActiveId((cur) => {
        if (cur !== e.payload.id) return cur;
        toast.info("Disconnected", e.payload.reason || "The server closed the connection.");
        return null;
      });
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (localCwd) return;
    api.fsHome().then(setLocalCwd).catch((e) => toast.error("Could not load home", errText(e)));
  }, [localCwd]);

  // Reset the remote path whenever the active connection changes.
  useEffect(() => {
    clearSearch();
    if (!active) {
      setRemotePath("");
      return;
    }
    setRemotePath(initialRemotePath(active));
  }, [active?.id]);

  // Clearing the box exits recursive-search mode.
  useEffect(() => {
    if (!searchQuery.trim()) setRemoteSearch(null);
  }, [searchQuery]);

  const refreshLocal = useCallback(async () => {
    if (!localCwd) return;
    setLocalLoading(true);
    try {
      const list = await api.fsList(localCwd, settings.showHidden);
      setLocalFiles(list.map(localToFileNode));
    } catch (e) {
      toast.error("Could not read folder", errText(e));
      setLocalFiles([]);
    } finally {
      setLocalLoading(false);
    }
  }, [localCwd, settings.showHidden]);

  const refreshRemote = useCallback(async () => {
    setRemoteFiles([]);
    if (!active) return;
    setRemoteLoading(true);
    try {
      const objs = await api.remoteList(active.id, remotePath);
      setRemoteFiles(objs.map(objectToFileNode));
    } catch (e) {
      toast.error("Could not list remote", errText(e));
    } finally {
      setRemoteLoading(false);
    }
  }, [active, remotePath]);

  useEffect(() => {
    refreshLocal();
  }, [refreshLocal]);
  useEffect(() => {
    refreshRemote();
  }, [refreshRemote]);

  // --- Local navigation ---
  const localPath = useMemo(() => splitLocalPath(localCwd), [localCwd]);
  const onLocalNavigate = (_seg: string, index: number) => {
    if (!localCwd) return;
    setSearchQuery("");
    const segs = splitLocalPath(localCwd);
    setLocalCwd(joinLocalPath(segs.slice(0, index + 1)));
  };
  const onLocalOpenFolder = (folderName: string) => {
    setSearchQuery("");
    const sep = localCwd.includes("\\") ? "\\" : "/";
    setLocalCwd(`${localCwd.replace(/[/\\]+$/, "")}${sep}${folderName}`);
  };
  const goLocalParent = async () => {
    if (!localCwd) return;
    setSearchQuery("");
    const p = await api.fsParent(localCwd);
    if (p && p !== localCwd) setLocalCwd(p);
  };

  // --- Remote navigation ---
  const remoteSegments = useMemo(
    () => splitRemotePath(active, remotePath),
    [active, remotePath],
  );
  const onRemoteOpenFolder = (folderName: string) => {
    // In search mode `folderName` is a path relative to the search root, which
    // joinRemoteSegment reconstructs into the correct absolute path.
    setRemotePath((p) => joinRemoteSegment(active, p, folderName));
    clearSearch();
  };
  const onRemoteNavigate = (_seg: string, index: number) => {
    if (remoteSearch) {
      clearSearch();
      return;
    }
    setRemotePath(remoteFromSegments(active, remoteSegments.slice(0, index + 1)));
  };
  const goRemoteParent = () => {
    if (remoteSearch) {
      clearSearch();
      return;
    }
    if (remoteSegments.length === 0) return;
    setRemotePath(remoteFromSegments(active, remoteSegments.slice(0, -1)));
  };

  const runRemoteSearch = useCallback(async () => {
    if (!active) {
      toast.error("No connection", "Pick a connection first.");
      return;
    }
    const q = searchQuery.trim();
    if (!q) {
      setRemoteSearch(null);
      return;
    }
    setRemoteSearching(true);
    try {
      const objs = await api.remoteSearch(active.id, remotePath, q);
      setRemoteSearch({ term: q, results: objs.map(objectToFileNode) });
    } catch (e) {
      toast.error("Search failed", errText(e));
    } finally {
      setRemoteSearching(false);
    }
  }, [active, remotePath, searchQuery]);

  // --- Actions ---
  const uploadFiles = async (fs: FileNode[]) => {
    if (!active) {
      toast.error("No connection", "Pick a connection first.");
      return;
    }
    const sep = localCwd.includes("\\") ? "\\" : "/";
    let count = 0;
    // Files and folders both go through remoteUpload — the backend detects a
    // directory source and uploads it recursively (creating remote dirs).
    for (const f of fs) {
      const src = `${localCwd.replace(/[/\\]+$/, "")}${sep}${f.name}`;
      const target = joinRemoteSegment(active, remotePath, f.name);
      try {
        await api.remoteUpload(active.id, src, target);
        count++;
      } catch (e) {
        toast.error(`Upload failed: ${f.name}`, errText(e));
      }
    }
    if (count > 0) {
      toast.success(`Uploaded ${count} item${count > 1 ? "s" : ""}`);
      refreshRemote();
    }
  };

  // Upload arbitrary local files (from a picker) into the active remote folder.
  const uploadPaths = async (srcPaths: string[]) => {
    if (!active) {
      toast.error("No connection", "Pick a connection first.");
      return;
    }
    let count = 0;
    for (const src of srcPaths) {
      const name = src.split(/[\\/]/).filter(Boolean).pop();
      if (!name) continue;
      const target = joinRemoteSegment(active, remotePath, name);
      try {
        await api.remoteUpload(active.id, src, target);
        count++;
      } catch (e) {
        toast.error(`Upload failed: ${name}`, errText(e));
      }
    }
    if (count > 0) {
      toast.success(`Uploaded ${count} item${count > 1 ? "s" : ""}`);
      refreshRemote();
    }
  };

  // Top-bar Upload button: open a small menu to choose files or a folder, since
  // a single native dialog can't offer both at once.
  const onUploadClick = (e: React.MouseEvent) => {
    if (!active) {
      toast.error("No connection", "Pick a connection first.");
      return;
    }
    setUploadMenu({ x: e.clientX, y: e.clientY });
  };

  const pickAndUpload = async (directory: boolean) => {
    setUploadMenu(null);
    const picked = await openDialog({ directory, multiple: true });
    if (!picked) return;
    await uploadPaths(Array.isArray(picked) ? picked : [picked]);
  };

  const downloadFiles = async (fs: FileNode[]) => {
    if (!active) return;
    const files = fs.filter((f) => f.kind === "file");
    if (files.length === 0) return;

    let baseDir = settings.defaultDownloadDir;
    if (files.length === 1 && !baseDir) {
      const dest = await saveDialog({ defaultPath: baseName(files[0].name) });
      if (!dest) return;
      const remote = joinRemoteSegment(active, remotePath, files[0].name);
      try {
        await api.remoteDownload(active.id, remote, dest);
        toast.success("Downloaded", files[0].name);
        refreshLocal();
      } catch (e) {
        toast.error("Download failed", errText(e));
      }
      return;
    }
    if (!baseDir) {
      const picked = await openDialog({ directory: true, multiple: false });
      if (typeof picked !== "string") return;
      baseDir = picked;
    }
    const dirSep = baseDir.includes("\\") ? "\\" : "/";
    let count = 0;
    for (const f of files) {
      const remote = joinRemoteSegment(active, remotePath, f.name);
      const dest = `${baseDir.replace(/[/\\]+$/, "")}${dirSep}${baseName(f.name)}`;
      try {
        await api.remoteDownload(active.id, remote, dest);
        count++;
      } catch (e) {
        toast.error(`Download failed: ${f.name}`, errText(e));
      }
    }
    if (count > 0) {
      toast.success(`Downloaded ${count} items`, `to ${baseDir}`);
      refreshLocal();
    }
  };

  const deleteRemote = (fs: FileNode[]) => {
    if (!active) return;
    setConfirm({
      title: `Delete ${fs.length} item${fs.length > 1 ? "s" : ""}?`,
      message: "This cannot be undone.",
      onConfirm: async () => {
        setConfirm(null);
        let count = 0;
        let skippedFolders = 0;
        for (const f of fs) {
          // Skip folders on virtual-bucket backends (S3 folders aren't real and
          // would need recursive delete); SFTP/FTP delete handles single dirs.
          if (f.kind === "folder" && active.caps?.virtualBuckets) {
            skippedFolders++;
            continue;
          }
          const target = joinRemoteSegment(active, remotePath, f.name);
          try {
            await api.remoteDelete(active.id, target);
            count++;
          } catch (e) {
            toast.error(`Delete failed: ${f.name}`, errText(e));
          }
        }
        if (count > 0) toast.success(`Deleted ${count} item${count > 1 ? "s" : ""}`);
        if (skippedFolders > 0) {
          toast.info(
            `Skipped ${skippedFolders} folder${skippedFolders > 1 ? "s" : ""}`,
            "Deleting S3 folders isn't supported yet — delete their contents first.",
          );
        }
        refreshRemote();
      },
    });
  };

  const copyRemotePath = async (f: FileNode) => {
    if (!active) return;
    const target = joinRemoteSegment(active, remotePath, f.name);
    const full = displayRemoteFull(active, target);
    try {
      await writeText(full);
      toast.success("Copied", full);
    } catch (e) {
      toast.error("Copy failed", errText(e));
    }
  };

  const copyLocalPath = async (f: FileNode) => {
    const sep = localCwd.includes("\\") ? "\\" : "/";
    const full = `${localCwd.replace(/[/\\]+$/, "")}${sep}${f.name}`;
    try {
      await writeText(full);
      toast.success("Copied", full);
    } catch (e) {
      toast.error("Copy failed", errText(e));
    }
  };

  // --- Global shortcuts ---
  useShortcuts([
    { shortcut: { key: "n", mods: ["mod"], allowInInput: true }, handler: () => setConnectOpen(true) },
    { shortcut: { key: "r", mods: ["mod"], allowInInput: true }, handler: () => {
      refreshLocal();
      refreshRemote();
    }},
    { shortcut: { key: "F5", mods: [], allowInInput: true }, handler: () => {
      refreshLocal();
      refreshRemote();
    }},
    { shortcut: { key: ",", mods: ["mod"], allowInInput: true }, handler: () => setSettingsOpen(true) },
    { shortcut: { key: "/", mods: ["mod"], allowInInput: true }, handler: () => setSettingsOpen(true) },
    { shortcut: { key: "k", mods: ["mod"], allowInInput: true }, handler: () => searchRef.current?.focus() },
    { shortcut: { key: "1", mods: ["mod"] }, handler: () => setFocusedSide("local") },
    { shortcut: { key: "2", mods: ["mod"] }, handler: () => setFocusedSide("remote") },
    { shortcut: { key: "ArrowUp", mods: ["mod"] }, handler: () => {
      if (focusedSide === "local") goLocalParent();
      else goRemoteParent();
    }},
  ]);

  // --- Display (apply live filter to the focused pane) ---
  const localView = focusedSide === "local" ? filterByName(localFiles, searchQuery) : localFiles;
  const remoteView = remoteSearch
    ? remoteSearch.results
    : focusedSide === "remote"
      ? filterByName(remoteFiles, searchQuery)
      : remoteFiles;

  const remotePathDisplay = remoteSearch ? [`Results: “${remoteSearch.term}”`] : remoteSegments;
  const remoteSubtitle = remoteSearch
    ? `${remoteSearch.results.length} match${remoteSearch.results.length === 1 ? "" : "es"} in ${remotePath || "all buckets"}`
    : active
      ? remoteDescription(active, remotePath)
      : "No connection selected";
  const localEmpty = focusedSide === "local" && searchQuery.trim() ? "No matches." : undefined;
  const remoteEmpty = !active
    ? "Add or pick a connection from the sidebar."
    : remoteSearch || (focusedSide === "remote" && searchQuery.trim())
      ? "No matches."
      : "Empty folder.";

  return (
    <WindowFrame>
      <TitleBar
        onConnect={() => setConnectOpen(true)}
        connectedTo={
          active
            ? `${active.username ?? ""}${active.username ? "@" : ""}${active.host ?? active.endpoint ?? ""}`
            : undefined
        }
        onDisconnect={active ? disconnectConnection : undefined}
        onRefresh={() => {
          refreshLocal();
          refreshRemote();
        }}
        onSettings={() => setSettingsOpen(true)}
        refreshing={localLoading || remoteLoading || remoteSearching}
        search={searchQuery}
        onSearchChange={setSearchQuery}
        onSearchSubmit={() => {
          if (focusedSide === "remote") runRemoteSearch();
        }}
        searchRef={searchRef}
        searchHint={focusedSide === "remote" && !remoteSearch ? "↵ subfolders" : undefined}
        searching={remoteSearching}
        onUpload={onUploadClick}
      />

      <PanelGroup direction="horizontal" className="flex-1 overflow-hidden" autoSaveId="cirrus.shell">
        <Panel defaultSize={20} minSize={14} maxSize={45}>
          <Sidebar
            connections={connections}
            connectedId={activeId}
            selectedId={selectedId}
            connectingId={connectingId}
            onSelect={selectConnection}
            onConnect={connectConnection}
            onContextMenu={(c, x, y) => setConnMenu({ c, x, y })}
            onNewConnection={() => setConnectOpen(true)}
            loading={connectionsLoading}
            error={connectionsError}
            onRetry={refresh}
          />
        </Panel>
        <VHandle />
        <Panel minSize={30}>
          <PanelGroup direction="vertical" autoSaveId="cirrus.main">
            <Panel defaultSize={72} minSize={20}>
              <PanelGroup direction="horizontal" className="h-full" autoSaveId="cirrus.panes">
                <Panel defaultSize={50} minSize={15}>
                  <FilePane
                    side="local"
                    title="Local"
                    subtitle={localCwd || "Loading…"}
                    path={localPath}
                    files={localView}
                    loading={localLoading}
                    focused={focusedSide === "local"}
                    onFocus={() => setFocusedSide("local")}
                    onNavigate={onLocalNavigate}
                    onOpenFolder={onLocalOpenFolder}
                    onActivate={(f) => uploadFiles([f])}
                    onAction={uploadFiles}
                    onCopyPath={copyLocalPath}
                    emptyMessage={localEmpty}
                  />
                </Panel>
                <VHandle />
                <Panel defaultSize={50} minSize={15}>
                  <FilePane
                    side="remote"
                    title={`Remote · ${active ? KIND_LABEL[active.kind] : "—"}`}
                    subtitle={remoteSubtitle}
                    path={remotePathDisplay}
                    files={remoteView}
                    loading={remoteLoading || remoteSearching}
                    focused={focusedSide === "remote"}
                    onFocus={() => setFocusedSide("remote")}
                    onNavigate={onRemoteNavigate}
                    onOpenFolder={onRemoteOpenFolder}
                    onActivate={(f) => downloadFiles([f])}
                    onAction={downloadFiles}
                    onDelete={deleteRemote}
                    onCopyPath={copyRemotePath}
                    emptyMessage={remoteEmpty}
                  />
                </Panel>
              </PanelGroup>
            </Panel>
            <HHandle />
            <Panel defaultSize={28} minSize={5} maxSize={80}>
              <TransferQueue />
            </Panel>
          </PanelGroup>
        </Panel>
      </PanelGroup>

      <ConnectSheet
        open={connectOpen}
        onClose={() => setConnectOpen(false)}
        onSaved={async (c) => {
          await refresh();
          setSelectedId(c.id);
          setActiveId(c.id);
          setConnectOpen(false);
          toast.success("Connection saved", c.name);
        }}
      />

      <SettingsSheet
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        showHidden={settings.showHidden}
        // refreshLocal depends on settings.showHidden, so the list reloads automatically.
        onShowHiddenChange={setSet.setShowHidden}
        defaultDownloadDir={settings.defaultDownloadDir}
        onDefaultDownloadDirChange={setSet.setDefaultDownloadDir}
      />

      <ConfirmDialog
        open={!!confirm}
        title={confirm?.title ?? ""}
        message={confirm?.message}
        confirmLabel="Delete"
        danger
        onConfirm={() => confirm?.onConfirm()}
        onCancel={() => setConfirm(null)}
      />

      <ContextMenu
        open={!!connMenu}
        x={connMenu?.x ?? 0}
        y={connMenu?.y ?? 0}
        items={
          connMenu
            ? connMenu.c.id === activeId
              ? [
                  {
                    type: "item",
                    label: "Reconnect",
                    icon: <Plug className="h-3.5 w-3.5" />,
                    onClick: () => connectConnection(connMenu.c),
                  },
                  {
                    type: "item",
                    label: "Disconnect",
                    icon: <Unplug className="h-3.5 w-3.5" />,
                    danger: true,
                    onClick: disconnectConnection,
                  },
                ]
              : [
                  {
                    type: "item",
                    label: "Connect",
                    icon: <Plug className="h-3.5 w-3.5" />,
                    onClick: () => connectConnection(connMenu.c),
                  },
                ]
            : []
        }
        onClose={() => setConnMenu(null)}
      />

      <ContextMenu
        open={!!uploadMenu}
        x={uploadMenu?.x ?? 0}
        y={uploadMenu?.y ?? 0}
        items={[
          {
            type: "item",
            label: "Upload files…",
            icon: <FileUp className="h-3.5 w-3.5" />,
            onClick: () => pickAndUpload(false),
          },
          {
            type: "item",
            label: "Upload folder…",
            icon: <FolderUp className="h-3.5 w-3.5" />,
            onClick: () => pickAndUpload(true),
          },
        ]}
        onClose={() => setUploadMenu(null)}
      />
    </WindowFrame>
  );
}

function filterByName(list: FileNode[], query: string): FileNode[] {
  const q = query.trim().toLowerCase();
  if (!q) return list;
  return list.filter((f) => f.name.toLowerCase().includes(q));
}

/** Last path segment, for both "/" and "\" separators. */
function baseName(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() ?? p;
}

// ----- Path helpers (local) -----

function splitLocalPath(p: string): string[] {
  if (!p) return [];
  if (p === "/") return ["/"];
  const isWin = /^[A-Za-z]:[\\/]/.test(p);
  if (isWin) return p.split(/[\\/]+/).filter(Boolean);
  const parts = p.split("/").filter(Boolean);
  return ["/", ...parts];
}

function joinLocalPath(parts: string[]): string {
  if (parts.length === 0) return "/";
  if (parts[0] === "/") return "/" + parts.slice(1).join("/");
  if (/^[A-Za-z]:$/.test(parts[0])) return parts[0] + "\\" + parts.slice(1).join("\\");
  return parts.join("/");
}

// ----- Path helpers (remote) -----

function initialRemotePath(c: Connection): string {
  if (c.caps?.virtualBuckets) {
    if (c.bucket && c.defaultPath) {
      return `${c.bucket}/${c.defaultPath.replace(/^\/+/, "")}`;
    }
    if (c.bucket) return c.bucket;
    return "";
  }
  return c.defaultPath || "";
}

function splitRemotePath(c: Connection | null, path: string): string[] {
  if (!c) return [];
  if (c.caps?.virtualBuckets) {
    if (!path) return ["(buckets)"];
    return path.split("/").filter(Boolean);
  }
  // SFTP/FTP
  if (!path || path === "/") return ["/"];
  return ["/", ...path.split("/").filter(Boolean)];
}

function remoteFromSegments(c: Connection | null, segs: string[]): string {
  if (!c) return "";
  if (c.caps?.virtualBuckets) {
    if (segs.length === 0 || segs[0] === "(buckets)") return "";
    return segs.join("/");
  }
  if (segs.length === 0) return "";
  if (segs[0] === "/") return "/" + segs.slice(1).join("/");
  return "/" + segs.join("/");
}

function joinRemoteSegment(c: Connection | null, base: string, name: string): string {
  if (!c) return name;
  if (c.caps?.virtualBuckets) {
    return base ? `${base.replace(/\/$/, "")}/${name}` : name;
  }
  if (!base || base === "/") return `/${name}`;
  return `${base.replace(/\/+$/, "")}/${name}`;
}

function displayRemoteFull(c: Connection, path: string): string {
  if (isS3Family(c.kind)) return path ? `s3://${path}` : "s3://";
  const proto = c.kind === "sftp" ? "sftp" : c.kind === "ftp" ? "ftp" : "ftps";
  const host = c.host ?? "";
  const cleaned = path.startsWith("/") ? path : `/${path}`;
  return `${proto}://${host}${cleaned}`;
}

function remoteDescription(c: Connection, path: string): string {
  if (isS3Family(c.kind)) {
    if (!path) return `${KIND_LABEL[c.kind]} · all buckets`;
    return `${KIND_LABEL[c.kind]} · ${path}`;
  }
  return `${KIND_LABEL[c.kind]} · ${c.username ?? ""}@${c.host ?? ""}:${path || "/"}`;
}

// ----- Mapping -----

function objectToFileNode(o: ObjectEntry): FileNode {
  const ext = o.name.includes(".") ? o.name.split(".").pop() : undefined;
  return {
    name: o.name,
    kind: o.kind,
    size: o.size ?? undefined,
    ext,
    modified: o.modified ? new Date(o.modified).toLocaleString() : "",
  };
}

function localToFileNode(e: LocalEntry): FileNode {
  const ext = e.name.includes(".") ? e.name.split(".").pop() : undefined;
  return {
    name: e.name,
    kind: e.kind,
    size: e.size ?? undefined,
    ext,
    modified: e.modified ? new Date(e.modified).toLocaleString() : "",
  };
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Folder,
  FileText,
  FileImage,
  FileCode,
  FileType,
  ChevronRight,
  ArrowUp,
  ArrowDown,
  Download,
  Upload,
  Trash2,
  Copy,
  ChevronUp,
  ChevronDown as ChevDown,
} from "lucide-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { type FileNode, formatBytes } from "@/lib/cirrus/types";
import { ContextMenu, type MenuItem } from "./ContextMenu";

export type SortKey = "name" | "size" | "modified";
export type SortDir = "asc" | "desc";

type Props = {
  side: "local" | "remote";
  title: string;
  subtitle: string;
  path: string[];
  files: FileNode[];
  loading?: boolean;
  focused?: boolean;
  onFocus?: () => void;
  onNavigate?: (segment: string, index: number) => void;
  onOpenFolder?: (folderName: string) => void;
  /** Called when an item is "activated" (double-click or Enter). */
  onActivate?: (file: FileNode) => void;
  /** Called for the row's primary action (download/upload). */
  onAction?: (files: FileNode[]) => void;
  onDelete?: (files: FileNode[]) => void;
  onCopyPath?: (file: FileNode) => void;
  emptyMessage?: string;
  rightHeaderSlot?: React.ReactNode;
};

export function FilePane({
  side,
  title,
  subtitle,
  path,
  files,
  loading,
  focused,
  onFocus,
  onNavigate,
  onOpenFolder,
  onActivate,
  onAction,
  onDelete,
  onCopyPath,
  emptyMessage,
  rightHeaderSlot,
}: Props) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; file: FileNode } | null>(null);
  const [sortKey, setSortKey] = useState<SortKey>("name");
  const [sortDir, setSortDir] = useState<SortDir>("asc");
  const listRef = useRef<HTMLDivElement | null>(null);

  // Clear selection and scroll back to the top when navigating elsewhere.
  useEffect(() => {
    setSelected(new Set());
    setAnchor(null);
    listRef.current?.scrollTo({ top: 0 });
  }, [path.join("/"), side]);

  const sorted = useMemo(() => sortFiles(files, sortKey, sortDir), [files, sortKey, sortDir]);
  const indexOf = useCallback(
    (name: string) => sorted.findIndex((f) => f.name === name),
    [sorted],
  );

  const virtualizer = useVirtualizer({
    count: sorted.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => 30,
    overscan: 12,
  });

  const selectedFiles = useMemo(
    () => sorted.filter((f) => selected.has(f.name)),
    [sorted, selected],
  );

  const click = (f: FileNode, e: React.MouseEvent) => {
    onFocus?.();
    if (e.shiftKey && anchor) {
      const a = indexOf(anchor);
      const b = indexOf(f.name);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a <= b ? [a, b] : [b, a];
        const next = new Set<string>();
        for (let i = lo; i <= hi; i++) next.add(sorted[i].name);
        setSelected(next);
        return;
      }
    }
    if (e.metaKey || e.ctrlKey) {
      const next = new Set(selected);
      if (next.has(f.name)) next.delete(f.name);
      else next.add(f.name);
      setSelected(next);
      setAnchor(f.name);
      return;
    }
    setSelected(new Set([f.name]));
    setAnchor(f.name);
  };

  const moveTo = (idx: number, extend: boolean) => {
    const target = sorted[Math.max(0, Math.min(idx, sorted.length - 1))];
    if (!target) return;
    setAnchor(target.name);
    if (extend) setSelected((s) => new Set([...s, target.name]));
    else setSelected(new Set([target.name]));
    virtualizer.scrollToIndex(indexOf(target.name), { align: "auto" });
  };

  const onRowKeyDown = (e: React.KeyboardEvent) => {
    if (sorted.length === 0) return;
    const curName = anchor && selected.has(anchor) ? anchor : sorted[0]?.name;
    const idx = curName ? indexOf(curName) : -1;

    if (e.key === "ArrowDown") {
      e.preventDefault();
      moveTo(idx + 1, e.shiftKey);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      moveTo(idx - 1, e.shiftKey);
    } else if (e.key === "Home") {
      e.preventDefault();
      moveTo(0, e.shiftKey);
    } else if (e.key === "End") {
      e.preventDefault();
      moveTo(sorted.length - 1, e.shiftKey);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const current = sorted.find((f) => selected.has(f.name)) ?? (anchor ? sorted.find((f) => f.name === anchor) : null);
      if (current) {
        if (current.kind === "folder") onOpenFolder?.(current.name);
        else onActivate?.(current);
      }
    } else if (e.key === "a" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      setSelected(new Set(sorted.map((f) => f.name)));
    } else if ((e.key === "Backspace" && (e.metaKey || e.ctrlKey)) || e.key === "Delete") {
      if (onDelete && selectedFiles.length > 0) {
        e.preventDefault();
        onDelete(selectedFiles);
      }
    }
  };

  const buildMenu = (f: FileNode): MenuItem[] => {
    const multiple = selectedFiles.length > 1 && selected.has(f.name);
    const targets = multiple ? selectedFiles : [f];
    const items: MenuItem[] = [];
    if (f.kind === "folder" && !multiple) {
      items.push({
        type: "item",
        label: "Open",
        icon: <Folder className="h-3.5 w-3.5" />,
        onClick: () => onOpenFolder?.(f.name),
        shortcut: "↵",
      });
      if (side === "local") {
        items.push({
          type: "item",
          label: "Upload folder",
          icon: <Upload className="h-3.5 w-3.5" />,
          onClick: () => onAction?.([f]),
        });
      }
    } else {
      items.push({
        type: "item",
        label: side === "local" ? (multiple ? `Upload ${targets.length} items` : "Upload") : multiple ? `Download ${targets.length} items` : "Download",
        icon: side === "local" ? <Upload className="h-3.5 w-3.5" /> : <Download className="h-3.5 w-3.5" />,
        onClick: () => onAction?.(targets),
        shortcut: "↵",
      });
    }
    if (onCopyPath && !multiple) {
      items.push({ type: "item", label: "Copy path", icon: <Copy className="h-3.5 w-3.5" />, onClick: () => onCopyPath(f) });
    }
    if (onDelete) {
      items.push({ type: "separator" });
      items.push({
        type: "item",
        label: multiple ? `Delete ${targets.length} items` : "Delete",
        icon: <Trash2 className="h-3.5 w-3.5" />,
        onClick: () => onDelete(targets),
        danger: true,
        shortcut: "⌘⌫",
      });
    }
    return items;
  };

  return (
    <div
      tabIndex={0}
      role="group"
      aria-label={`${title} pane`}
      onFocus={() => onFocus?.()}
      onClick={() => onFocus?.()}
      onKeyDown={onRowKeyDown}
      className={`flex h-full flex-1 flex-col bg-card/40 outline-none transition-shadow ${
        focused ? "ring-1 ring-inset ring-primary/40" : ""
      }`}
    >
      <div className="flex items-center justify-between border-b border-border/60 px-3 py-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                loading ? "animate-pulse bg-warning" : side === "local" ? "bg-foreground/40" : "bg-success"
              }`}
            />
            {title}
          </div>
          <div className="mt-0.5 truncate text-[12px] text-muted-foreground" title={subtitle}>{subtitle}</div>
        </div>
        {rightHeaderSlot && <div className="flex items-center gap-1">{rightHeaderSlot}</div>}
      </div>

      <nav
        aria-label={`${title} path`}
        className="flex items-center gap-1 overflow-x-auto border-b border-border/40 bg-background/30 px-3 py-1.5 text-[12px] text-muted-foreground scrollbar-mac"
      >
        {path.map((p, i) => (
          <span key={i} className="flex shrink-0 items-center gap-1">
            <button
              onClick={() => onNavigate?.(p, i)}
              className="rounded px-1 py-0.5 text-foreground/80 hover:bg-foreground/5"
            >
              {p}
            </button>
            {i < path.length - 1 && <ChevronRight className="h-3 w-3 opacity-50" />}
          </span>
        ))}
      </nav>

      <div className="grid grid-cols-[1fr_90px_140px] gap-3 border-b border-border/40 px-3 py-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/80">
        <ColHeader k="name" sortKey={sortKey} sortDir={sortDir} onSort={setSort(setSortKey, setSortDir, sortKey, sortDir)}>
          Name
        </ColHeader>
        <ColHeader k="size" align="right" sortKey={sortKey} sortDir={sortDir} onSort={setSort(setSortKey, setSortDir, sortKey, sortDir)}>
          Size
        </ColHeader>
        <ColHeader k="modified" sortKey={sortKey} sortDir={sortDir} onSort={setSort(setSortKey, setSortDir, sortKey, sortDir)}>
          Modified
        </ColHeader>
      </div>

      <div
        ref={listRef}
        role="listbox"
        aria-label={`${title} files`}
        aria-multiselectable="true"
        className="flex-1 overflow-y-auto scrollbar-mac"
      >
        {sorted.length === 0 && (
          <div className="flex h-full items-center justify-center px-6 py-10 text-center text-[12px] text-muted-foreground">
            {loading ? "Loading…" : (emptyMessage ?? "Empty")}
          </div>
        )}
        {sorted.length > 0 && (
          <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((vi) => {
              const f = sorted[vi.index];
              const active = selected.has(f.name);
              return (
                <div
                  key={f.name}
                  data-index={vi.index}
                  ref={virtualizer.measureElement}
                  role="option"
                  aria-selected={active}
                  onMouseDown={(e) => click(f, e)}
                  onDoubleClick={() => {
                    if (f.kind === "folder") onOpenFolder?.(f.name);
                    else onActivate?.(f);
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    if (!selected.has(f.name)) {
                      setSelected(new Set([f.name]));
                      setAnchor(f.name);
                    }
                    onFocus?.();
                    setMenu({ x: e.clientX, y: e.clientY, file: f });
                  }}
                  style={{ transform: `translateY(${vi.start}px)` }}
                  className={`absolute left-0 top-0 grid w-full cursor-default grid-cols-[1fr_90px_140px] items-center gap-3 px-3 py-1.5 text-left text-[13px] ${
                    active
                      ? focused
                        ? "bg-primary/15 text-foreground"
                        : "bg-primary/8 text-foreground"
                      : vi.index % 2 === 0
                        ? "hover:bg-foreground/[0.03]"
                        : "bg-foreground/[0.015] hover:bg-foreground/[0.04]"
                  }`}
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <FileIcon node={f} />
                    <span className="truncate">{f.name}</span>
                  </span>
                  <span className="text-right font-mono text-[12px] text-muted-foreground">
                    {f.kind === "folder" ? "—" : formatBytes(f.size ?? 0)}
                  </span>
                  <span className="truncate text-[12px] text-muted-foreground">{f.modified}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <div className="flex items-center justify-between border-t border-border/60 bg-background/30 px-3 py-1.5 text-[11px] text-muted-foreground">
        <span aria-live="polite">
          {selectedFiles.length > 0
            ? `${selectedFiles.length} of ${sorted.length} selected`
            : `${sorted.length} items`}
        </span>
        <button
          onClick={() => selectedFiles.length > 0 && onAction?.(selectedFiles)}
          disabled={selectedFiles.length === 0}
          className="flex items-center gap-1 rounded px-1 py-0.5 disabled:opacity-50 hover:bg-foreground/5 disabled:hover:bg-transparent"
        >
          {side === "local" ? (
            <ArrowUp className="h-3 w-3 text-primary" />
          ) : (
            <ArrowDown className="h-3 w-3 text-primary" />
          )}
          {side === "local" ? "Upload selected" : "Download selected"}
        </button>
      </div>

      <ContextMenu
        open={!!menu}
        x={menu?.x ?? 0}
        y={menu?.y ?? 0}
        items={menu ? buildMenu(menu.file) : []}
        onClose={() => setMenu(null)}
      />
    </div>
  );
}

function setSort(
  setKey: (k: SortKey) => void,
  setDir: (d: SortDir) => void,
  curKey: SortKey,
  curDir: SortDir,
) {
  return (k: SortKey) => {
    if (k === curKey) setDir(curDir === "asc" ? "desc" : "asc");
    else {
      setKey(k);
      setDir("asc");
    }
  };
}

function ColHeader({
  k,
  sortKey,
  sortDir,
  onSort,
  align,
  children,
}: {
  k: SortKey;
  sortKey: SortKey;
  sortDir: SortDir;
  onSort: (k: SortKey) => void;
  align?: "right";
  children: React.ReactNode;
}) {
  const active = sortKey === k;
  return (
    <button
      onClick={() => onSort(k)}
      aria-sort={active ? (sortDir === "asc" ? "ascending" : "descending") : undefined}
      aria-label={`Sort by ${k}`}
      className={`flex items-center gap-1 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wider transition hover:bg-foreground/5 ${
        active ? "text-foreground" : "text-muted-foreground/80"
      } ${align === "right" ? "justify-end" : ""}`}
    >
      {align === "right" && active && (sortDir === "asc" ? <ChevronUp className="h-3 w-3" /> : <ChevDown className="h-3 w-3" />)}
      {children}
      {align !== "right" && active && (sortDir === "asc" ? <ChevronUp className="h-3 w-3" /> : <ChevDown className="h-3 w-3" />)}
    </button>
  );
}

function sortFiles(files: FileNode[], key: SortKey, dir: SortDir): FileNode[] {
  const mult = dir === "asc" ? 1 : -1;
  const folderFirst = (a: FileNode, b: FileNode) => (a.kind === b.kind ? 0 : a.kind === "folder" ? -1 : 1);
  return [...files].sort((a, b) => {
    const fp = folderFirst(a, b);
    if (fp !== 0) return fp;
    if (key === "size") return ((a.size ?? 0) - (b.size ?? 0)) * mult;
    if (key === "modified") return (a.modified ?? "").localeCompare(b.modified ?? "") * mult;
    return a.name.toLowerCase().localeCompare(b.name.toLowerCase()) * mult;
  });
}

const ICON_MAP: Record<string, React.ComponentType<{ className?: string }>> = {
  png: FileImage,
  jpg: FileImage,
  jpeg: FileImage,
  ico: FileImage,
  webp: FileImage,
  gif: FileImage,
  svg: FileImage,
  md: FileText,
  txt: FileText,
  log: FileText,
  pdf: FileType,
  yaml: FileCode,
  yml: FileCode,
  sh: FileCode,
  js: FileCode,
  ts: FileCode,
  tsx: FileCode,
  jsx: FileCode,
  json: FileCode,
  html: FileCode,
  css: FileCode,
  rs: FileCode,
};

function FileIcon({ node }: { node: FileNode }) {
  if (node.kind === "folder")
    return <Folder className="h-4 w-4 text-primary" fill="currentColor" fillOpacity={0.18} />;
  const Icon = ICON_MAP[node.ext ?? ""] ?? FileText;
  return <Icon className="h-4 w-4 text-muted-foreground" />;
}

import { useEffect, useState, type RefObject } from "react";
import {
  Search,
  RefreshCw,
  Plug,
  Unplug,
  Upload,
  Sun,
  Moon,
  Minus,
  Square,
  X as XIcon,
  Settings,
  Loader2,
} from "lucide-react";
import logo from "@/assets/cirrus-logo.png";
import { useTheme } from "@/hooks/use-theme";
import { win, getPlatform } from "@/lib/cirrus/ipc";

type Props = {
  onConnect: () => void;
  connectedTo?: string;
  onDisconnect?: () => void;
  onRefresh?: () => void;
  onSettings?: () => void;
  refreshing?: boolean;
  // Search — rendered only when onSearchChange is provided.
  search?: string;
  onSearchChange?: (v: string) => void;
  onSearchSubmit?: () => void;
  searchRef?: RefObject<HTMLInputElement | null>;
  searchPlaceholder?: string;
  searchHint?: string;
  searching?: boolean;
  // Upload to the active remote folder (opens a files/folder chooser menu).
  onUpload?: React.MouseEventHandler<HTMLButtonElement>;
};

export function TitleBar({
  onConnect,
  connectedTo,
  onDisconnect,
  onRefresh,
  onSettings,
  refreshing,
  search,
  onSearchChange,
  onSearchSubmit,
  searchRef,
  searchPlaceholder = "Search files…",
  searchHint,
  searching,
  onUpload,
}: Props) {
  const { theme, toggle } = useTheme();
  const [platform] = useState(getPlatform);
  const isMac = platform === "mac";

  return (
    <div
      data-tauri-drag-region
      onDoubleClick={() => win.toggleMaximize()}
      className="titlebar-bg relative flex h-12 items-center gap-3 border-b border-border/60 px-3 select-none"
    >
      {isMac && <MacTrafficLights />}

      <div className="flex items-center gap-1">
        <IconBtn onClick={onRefresh} title="Refresh (⌘R)" aria-label="Refresh both panes">
          <RefreshCw className={`h-4 w-4 ${refreshing ? "animate-spin" : ""}`} />
        </IconBtn>
      </div>

      <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2">
        <div className="flex items-center gap-2 text-[13px] font-medium text-foreground/80">
          <img src={logo} alt="Novacirrus" width={20} height={20} className="h-5 w-5 drop-shadow-sm" />
          <span className="font-display text-[14px] font-semibold tracking-tight">Novacirrus</span>
          {connectedTo && (
            <>
              <span className="text-muted-foreground/60">—</span>
              <span className="text-muted-foreground">{connectedTo}</span>
            </>
          )}
        </div>
      </div>

      <div className="ml-auto flex items-center gap-2">
        {onSearchChange && (
          <div className="hidden md:flex h-7 w-64 items-center gap-2 rounded-md border border-border/60 bg-card/60 px-2 text-xs text-muted-foreground focus-within:border-primary/60 focus-within:ring-1 focus-within:ring-primary/30">
            {searching ? (
              <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-primary" />
            ) : (
              <Search className="h-3.5 w-3.5 shrink-0" />
            )}
            <input
              ref={searchRef}
              aria-label="Search files"
              value={search ?? ""}
              onChange={(e) => onSearchChange(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  onSearchSubmit?.();
                } else if (e.key === "Escape" && search) {
                  e.preventDefault();
                  e.stopPropagation();
                  onSearchChange("");
                }
              }}
              placeholder={searchPlaceholder}
              className="min-w-0 flex-1 bg-transparent text-[12px] text-foreground placeholder:text-muted-foreground focus:outline-none"
            />
            {search ? (
              <button
                onClick={() => onSearchChange("")}
                aria-label="Clear search"
                className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-foreground/10 hover:text-foreground"
              >
                <XIcon className="h-3 w-3" />
              </button>
            ) : searchHint ? (
              <span className="shrink-0 whitespace-nowrap text-[10px] text-muted-foreground/70">{searchHint}</span>
            ) : (
              <span className="ml-auto shrink-0 rounded border border-border/60 px-1 font-mono text-[10px]">⌘K</span>
            )}
          </div>
        )}
        <button
          onClick={toggle}
          aria-label="Toggle theme"
          className="flex h-7 w-7 items-center justify-center rounded-md text-foreground/70 transition hover:bg-foreground/5 hover:text-foreground"
        >
          {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
        </button>
        {onDisconnect && (
          <button
            onClick={onDisconnect}
            title="Disconnect from the current remote"
            className="flex h-7 items-center gap-1.5 rounded-md border border-border/60 bg-card/60 px-2.5 text-[12px] font-medium text-foreground shadow-sm transition hover:border-destructive/50 hover:bg-destructive/10 hover:text-destructive active:scale-[0.98]"
          >
            <Unplug className="h-3.5 w-3.5" />
            Disconnect
          </button>
        )}
        <button
          onClick={onConnect}
          className="flex h-7 items-center gap-1.5 rounded-md bg-primary px-2.5 text-[12px] font-medium text-primary-foreground shadow-sm transition hover:brightness-110 active:scale-[0.98]"
        >
          <Plug className="h-3.5 w-3.5" />
          New Connection
        </button>
        <IconBtn onClick={onSettings} title="Settings (⌘,)" aria-label="Open settings">
          <Settings className="h-4 w-4" />
        </IconBtn>
        <IconBtn onClick={onUpload} title="Upload to current remote folder" aria-label="Upload to current remote folder">
          <Upload className="h-4 w-4" />
        </IconBtn>

        {!isMac && <WinControls />}
      </div>
    </div>
  );
}

function MacTrafficLights() {
  return (
    <div className="group/tl flex items-center gap-2 pl-1 pr-3">
      <TLight
        color="bg-[oklch(0.66_0.21_25)]"
        label="Close"
        onClick={() => win.close()}
        glyph={<XIcon className="h-2 w-2 stroke-[3] text-black/70 opacity-0 group-hover/tl:opacity-100" />}
      />
      <TLight
        color="bg-[oklch(0.82_0.16_80)]"
        label="Minimize"
        onClick={() => win.minimize()}
        glyph={<Minus className="h-2 w-2 stroke-[3] text-black/70 opacity-0 group-hover/tl:opacity-100" />}
      />
      <TLight
        color="bg-[oklch(0.74_0.17_145)]"
        label="Zoom"
        onClick={() => win.toggleMaximize()}
        glyph={<Square className="h-1.5 w-1.5 stroke-[3] text-black/70 opacity-0 group-hover/tl:opacity-100" />}
      />
    </div>
  );
}

function TLight({
  color,
  label,
  onClick,
  glyph,
}: {
  color: string;
  label: string;
  onClick: () => void;
  glyph: React.ReactNode;
}) {
  return (
    <button
      aria-label={label}
      onClick={onClick}
      className={`flex h-3 w-3 items-center justify-center rounded-full ${color} ring-1 ring-black/10 transition hover:brightness-110`}
    >
      {glyph}
    </button>
  );
}

function WinControls() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    win.isMaximized().then(setMaximized);
  }, []);
  return (
    <div className="ml-1 flex h-12 -my-0.5 -mr-3 items-stretch">
      <WinBtn onClick={() => win.minimize()} aria-label="Minimize">
        <Minus className="h-3.5 w-3.5" />
      </WinBtn>
      <WinBtn
        onClick={async () => {
          await win.toggleMaximize();
          setMaximized(await win.isMaximized());
        }}
        aria-label={maximized ? "Restore" : "Maximize"}
      >
        <Square className="h-3 w-3" />
      </WinBtn>
      <WinBtn onClick={() => win.close()} aria-label="Close" danger>
        <XIcon className="h-3.5 w-3.5" />
      </WinBtn>
    </div>
  );
}

function WinBtn({
  children,
  onClick,
  danger,
  ...rest
}: {
  children: React.ReactNode;
  onClick: () => void;
  danger?: boolean;
} & React.HTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      onClick={onClick}
      className={`flex w-11 items-center justify-center text-foreground/70 transition ${
        danger ? "hover:bg-destructive hover:text-destructive-foreground" : "hover:bg-foreground/10"
      }`}
      {...rest}
    >
      {children}
    </button>
  );
}

function IconBtn({
  children,
  onClick,
  title,
  ...rest
}: {
  children: React.ReactNode;
  onClick?: React.MouseEventHandler<HTMLButtonElement>;
  title?: string;
} & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      onClick={onClick}
      title={title}
      className="flex h-7 w-7 items-center justify-center rounded-md text-foreground/70 transition hover:bg-foreground/5 hover:text-foreground"
      {...rest}
    >
      {children}
    </button>
  );
}

import { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { X, Sun, Moon, FolderOpen } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useTheme } from "@/hooks/use-theme";
import { useModalEsc } from "@/hooks/use-modal-esc";
import { useFocusTrap } from "@/hooks/use-focus-trap";

type Props = {
  open: boolean;
  onClose: () => void;
  showHidden: boolean;
  onShowHiddenChange: (v: boolean) => void;
  defaultDownloadDir: string;
  onDefaultDownloadDirChange: (dir: string) => void;
};

export function SettingsSheet({
  open,
  onClose,
  showHidden,
  onShowHiddenChange,
  defaultDownloadDir,
  onDefaultDownloadDirChange,
}: Props) {
  const { theme, toggle } = useTheme();

  useModalEsc(open, onClose);
  const trapRef = useFocusTrap<HTMLDivElement>(open);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          className="fixed inset-0 z-50 flex items-start justify-center pt-16"
          onClick={onClose}
        >
          <div className="absolute inset-0 bg-black/30 backdrop-blur-sm" />

          <motion.div
            ref={trapRef}
            role="dialog"
            aria-modal="true"
            aria-label="Settings"
            tabIndex={-1}
            onClick={(e) => e.stopPropagation()}
            initial={{ y: -12, opacity: 0, scale: 0.98 }}
            animate={{ y: 0, opacity: 1, scale: 1 }}
            exit={{ y: -12, opacity: 0, scale: 0.98 }}
            transition={{ type: "spring", stiffness: 380, damping: 30 }}
            className="popover-shadow vibrancy relative w-[520px] overflow-hidden rounded-2xl border border-border/60 bg-popover/95 outline-none"
          >
            <div className="flex items-center justify-between border-b border-border/60 px-5 py-3">
              <div>
                <h2 className="font-display text-[15px] font-semibold tracking-tight">Settings</h2>
                <p className="text-[12px] text-muted-foreground">Preferences for Novacirrus</p>
              </div>
              <button
                onClick={onClose}
                aria-label="Close"
                className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/5 hover:text-foreground"
              >
                <X className="h-4 w-4" />
              </button>
            </div>

            <div className="space-y-5 px-5 py-4">
              <Section title="Appearance">
                <Row
                  label="Theme"
                  hint="Switch between light and dark"
                  control={
                    <button
                      onClick={toggle}
                      className="flex h-7 items-center gap-1.5 rounded-md border border-border/60 bg-card/60 px-2.5 text-[12px] hover:bg-foreground/5"
                    >
                      {theme === "dark" ? (
                        <>
                          <Moon className="h-3.5 w-3.5" /> Dark
                        </>
                      ) : (
                        <>
                          <Sun className="h-3.5 w-3.5" /> Light
                        </>
                      )}
                    </button>
                  }
                />
              </Section>

              <Section title="Local files">
                <Row
                  label="Show hidden files"
                  hint="Show dotfiles in the local pane"
                  control={
                    <Toggle value={showHidden} onChange={onShowHiddenChange} />
                  }
                />
                <Row
                  label="Default download location"
                  hint={defaultDownloadDir || "Always ask"}
                  control={
                    <button
                      onClick={async () => {
                        const dir = await openDialog({ directory: true, multiple: false });
                        if (typeof dir === "string") onDefaultDownloadDirChange(dir);
                      }}
                      className="flex h-7 items-center gap-1.5 rounded-md border border-border/60 bg-card/60 px-2.5 text-[12px] hover:bg-foreground/5"
                    >
                      <FolderOpen className="h-3.5 w-3.5" /> Choose…
                    </button>
                  }
                />
                {defaultDownloadDir && (
                  <Row
                    label=""
                    hint=""
                    control={
                      <button
                        onClick={() => onDefaultDownloadDirChange("")}
                        className="text-[11px] text-muted-foreground hover:text-foreground"
                      >
                        Reset
                      </button>
                    }
                  />
                )}
              </Section>

              <Section title="Keyboard shortcuts">
                <KbdRow label="New connection" keys={["⌘", "N"]} />
                <KbdRow label="Refresh" keys={["⌘", "R"]} />
                <KbdRow label="Settings & shortcuts" keys={["⌘", ","]} />
                <KbdRow label="Focus search" keys={["⌘", "K"]} />
                <KbdRow label="Focus local pane" keys={["⌘", "1"]} />
                <KbdRow label="Focus remote pane" keys={["⌘", "2"]} />
                <KbdRow label="Parent directory" keys={["⌘", "↑"]} />
                <KbdRow label="Open / activate" keys={["↵"]} />
                <KbdRow label="Select all" keys={["⌘", "A"]} />
                <KbdRow label="Delete" keys={["⌘", "⌫"]} />
                <KbdRow label="Cancel / close" keys={["⎋"]} />
              </Section>
            </div>

            <div className="flex items-center justify-end border-t border-border/60 bg-background/30 px-5 py-3">
              <button
                onClick={onClose}
                className="h-8 rounded-md bg-primary px-3 text-[12px] font-semibold text-primary-foreground shadow-sm transition hover:brightness-110 active:scale-[0.98]"
              >
                Done
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-2 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
        {title}
      </div>
      <div className="rounded-lg border border-border/60 bg-card/40 p-1">{children}</div>
    </div>
  );
}

function Row({
  label,
  hint,
  control,
}: {
  label: string;
  hint?: string;
  control: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-3 px-2 py-2">
      <div className="min-w-0 flex-1">
        {label && <div className="text-[13px] text-foreground">{label}</div>}
        {hint && <div className="truncate text-[11px] text-muted-foreground">{hint}</div>}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}

function Toggle({ value, onChange }: { value: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={value}
      onClick={() => onChange(!value)}
      className={`relative h-5 w-9 rounded-full transition ${
        value ? "bg-primary" : "bg-foreground/15"
      }`}
    >
      <span
        className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${
          value ? "translate-x-[18px]" : "translate-x-0.5"
        }`}
      />
    </button>
  );
}

function KbdRow({ label, keys }: { label: string; keys: string[] }) {
  return (
    <div className="flex items-center gap-3 px-2 py-1.5">
      <div className="flex-1 text-[12.5px] text-foreground">{label}</div>
      <div className="flex gap-1">
        {keys.map((k, i) => (
          <kbd
            key={i}
            className="flex h-5 min-w-[20px] items-center justify-center rounded border border-border/60 bg-card/80 px-1 font-mono text-[10.5px] text-foreground/80"
          >
            {k}
          </kbd>
        ))}
      </div>
    </div>
  );
}

// Local "settings" persistence in localStorage.
const KEY = "cirrus.settings";
export type AppSettings = { showHidden: boolean; defaultDownloadDir: string };
const DEFAULTS: AppSettings = { showHidden: false, defaultDownloadDir: "" };

export function loadSettings(): AppSettings {
  if (typeof window === "undefined") return DEFAULTS;
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return DEFAULTS;
  }
}

export function saveSettings(s: AppSettings) {
  if (typeof window === "undefined") return;
  localStorage.setItem(KEY, JSON.stringify(s));
}

export function useSettings() {
  const [s, setS] = useState<AppSettings>(loadSettings);
  useEffect(() => {
    saveSettings(s);
  }, [s]);
  return [
    s,
    {
      setShowHidden: (v: boolean) => setS((p) => ({ ...p, showHidden: v })),
      setDefaultDownloadDir: (v: string) => setS((p) => ({ ...p, defaultDownloadDir: v })),
    },
  ] as const;
}

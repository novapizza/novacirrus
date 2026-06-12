import { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { X, Server, Cloud, Lock, KeyRound } from "lucide-react";
import {
  type Connection,
  type ConnectionKind,
  type ConnectionSecret,
  KIND_LABEL,
  isS3Family,
} from "@/lib/cirrus/types";
import { api } from "@/lib/cirrus/ipc";
import { useModalEsc } from "@/hooks/use-modal-esc";
import { useFocusTrap } from "@/hooks/use-focus-trap";

type Props = {
  open: boolean;
  onClose: () => void;
  initial?: Connection | null;
  onSaved?: (c: Connection) => void;
};

const KINDS: { id: ConnectionKind; label: string; desc: string; icon: typeof Server }[] = [
  { id: "s3", label: "S3", desc: "Amazon S3", icon: Cloud },
  { id: "r2", label: "R2", desc: "Cloudflare R2", icon: Cloud },
  { id: "s3-compat", label: "Compat", desc: "MinIO, B2, Wasabi…", icon: Cloud },
  { id: "sftp", label: "SFTP", desc: "Secure SSH", icon: Lock },
  { id: "ftp", label: "FTP", desc: "Classic", icon: Server },
  { id: "ftps", label: "FTPS", desc: "FTP over TLS", icon: Lock },
];

const DEFAULT_COLOR = "oklch(0.62 0.19 255)";

/** A syntactically valid IPv4/IPv6 address, `localhost`, or a dotted domain. */
function isValidHost(host: string): boolean {
  if (host === "localhost") return true;
  // IPv4 with each octet in 0–255.
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(host)) {
    return host.split(".").every((o) => Number(o) <= 255);
  }
  // IPv6 (loose: hex groups and colons).
  if (host.includes(":")) return /^[0-9a-fA-F:]+$/.test(host) && host.split(":").length >= 3;
  // Domain: one or more labels then an alphabetic TLD (requires at least one dot).
  return /^(?=.{1,253}$)([a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,}$/.test(host);
}

/** A valid http(s) URL — used for the S3 endpoint override. */
function isValidUrl(url: string): boolean {
  try {
    const u = new URL(url);
    return u.protocol === "http:" || u.protocol === "https:";
  } catch {
    return false;
  }
}

function emptyConnection(): Connection {
  return {
    id: "",
    name: "",
    kind: "s3",
    host: "",
    port: null,
    region: "",
    endpoint: "",
    bucket: "",
    defaultPath: "",
    username: "",
    usePathStyle: false,
    ftpMode: "passive",
    ftpsImplicit: false,
    color: DEFAULT_COLOR,
  };
}

export function ConnectSheet({ open, onClose, initial, onSaved }: Props) {
  const [conn, setConn] = useState<Connection>(initial ?? emptyConnection());
  const [secret, setSecret] = useState<ConnectionSecret>({});
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<string | null>(null);
  // SFTP auth is either/or: a password OR a private key, never both.
  const [sshAuth, setSshAuth] = useState<"password" | "key">("password");
  // Per-kind drafts: what you typed under one kind is kept for that kind, but
  // never cloned into another. Switching tabs snapshots the current kind here
  // and restores the target kind's own draft (or a clean form if none yet).
  type Draft = { conn: Connection; secret: ConnectionSecret; sshAuth: "password" | "key" };
  const [drafts, setDrafts] = useState<Partial<Record<ConnectionKind, Draft>>>({});

  useModalEsc(open, onClose);
  const trapRef = useFocusTrap<HTMLDivElement>(open);

  useEffect(() => {
    if (open) {
      setConn(initial ?? emptyConnection());
      setSecret({});
      setError(null);
      setTestResult(null);
      setSshAuth("password");
      setDrafts({});
    }
  }, [open, initial]);

  // Switching method drops the other credential so we never submit both.
  const chooseSshAuth = (method: "password" | "key") => {
    setSshAuth(method);
    setSecret((s) =>
      method === "password"
        ? { ...s, privateKeyPem: "", passphrase: "" }
        : { ...s, password: "" },
    );
  };

  const update = <K extends keyof Connection>(key: K, value: Connection[K]) =>
    setConn((c) => ({ ...c, [key]: value }));

  // Switching kind keeps what you typed for the current kind (snapshotted into
  // drafts) and restores the target kind's own draft — values are never cloned
  // across kinds. Identity (id/createdAt/color) is preserved so an in-progress
  // edit still targets the same saved connection.
  const changeKind = (kind: ConnectionKind) => {
    if (kind === conn.kind) return;
    setDrafts((d) => ({ ...d, [conn.kind]: { conn, secret, sshAuth } }));
    const saved = drafts[kind];
    if (saved) {
      setConn(saved.conn);
      setSecret(saved.secret);
      setSshAuth(saved.sshAuth);
    } else {
      setConn((c) => ({ ...emptyConnection(), kind, id: c.id, createdAt: c.createdAt, color: c.color }));
      setSecret({});
      setSshAuth("password");
    }
    setError(null);
    setTestResult(null);
  };

  async function save({ test }: { test: boolean }) {
    if (!valid) return; // guarded; the buttons are also disabled while invalid
    setSaving(true);
    setError(null);
    setTestResult(null);
    try {
      const saved = await api.upsertConnection(conn, secret);
      if (test) {
        try {
          const msg = await api.testConnection(saved.id);
          setTestResult(msg);
        } catch (e) {
          setError(String(e));
          return;
        }
      }
      onSaved?.(saved);
      if (!test) onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  const showS3 = isS3Family(conn.kind);
  const showSsh = conn.kind === "sftp";
  const showFtp = conn.kind === "ftp" || conn.kind === "ftps";

  // Required fields per kind, so we never persist a blank/unusable connection.
  // On edit, secrets aren't reloaded into the form (they live in the keychain),
  // so credential fields are only required when creating a new connection.
  const isEdit = !!initial?.id;
  const t = (v?: string | null) => (v ?? "").trim();
  const problems: string[] = [];
  if (!t(conn.name)) problems.push("Name");
  if (showS3) {
    if (conn.kind === "r2" || conn.kind === "s3-compat") {
      if (!t(conn.endpoint)) problems.push("Endpoint URL");
      else if (!isValidUrl(t(conn.endpoint))) problems.push("a valid Endpoint URL (https://…)");
    }
    if (!isEdit) {
      if (!t(secret.accessKeyId)) problems.push("Access Key ID");
      if (!t(secret.secretAccessKey)) problems.push("Secret Access Key");
    }
  }
  if (showSsh || showFtp) {
    // Host must be a real IP or domain, not just any non-empty string.
    if (!t(conn.host)) problems.push("Host");
    else if (!isValidHost(t(conn.host))) problems.push("a valid Host (IP or domain)");
    // No anonymous / default credentials: an explicit username is always required.
    if (!t(conn.username)) problems.push("Username");
  }
  if (!isEdit) {
    // A real credential is required (default/empty is not accepted).
    if (showSsh) {
      if (sshAuth === "password" && !t(secret.password)) problems.push("Password");
      if (sshAuth === "key" && !t(secret.privateKeyPem)) problems.push("Private key");
    }
    if (showFtp && !t(secret.password)) problems.push("Password");
  }
  const valid = problems.length === 0;

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          className="absolute inset-0 z-50 flex items-start justify-center"
          onClick={onClose}
        >
          <div className="absolute inset-0 bg-black/30 backdrop-blur-sm" />

          <motion.div
            ref={trapRef}
            role="dialog"
            aria-modal="true"
            aria-label={initial?.id ? "Edit connection" : "New connection"}
            tabIndex={-1}
            initial={{ y: -12, opacity: 0, scale: 0.98 }}
            animate={{ y: 0, opacity: 1, scale: 1 }}
            exit={{ y: -12, opacity: 0, scale: 0.98 }}
            transition={{ type: "spring", stiffness: 380, damping: 30 }}
            onClick={(e) => e.stopPropagation()}
            className="popover-shadow vibrancy relative mt-16 w-[600px] overflow-hidden rounded-2xl border border-border/60 bg-popover/90 outline-none"
          >
            <div className="flex items-center justify-between border-b border-border/60 px-5 py-3">
              <div>
                <h2 className="font-display text-[15px] font-semibold tracking-tight">
                  {initial?.id ? "Edit connection" : "New connection"}
                </h2>
                <p className="text-[12px] text-muted-foreground">
                  Saved to your config; secrets stored in the OS keychain.
                </p>
              </div>
              <button
                onClick={onClose}
                aria-label="Close"
                className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/5 hover:text-foreground"
              >
                <X className="h-4 w-4" />
              </button>
            </div>

            <div className="grid grid-cols-6 gap-1.5 border-b border-border/40 p-3">
              {KINDS.map((p) => {
                const active = p.id === conn.kind;
                const Icon = p.icon;
                return (
                  <button
                    key={p.id}
                    onClick={() => changeKind(p.id)}
                    className={`relative flex flex-col items-center gap-1 rounded-lg border px-2 py-2.5 text-center transition ${
                      active
                        ? "border-primary/50 bg-primary/10 text-foreground"
                        : "border-border/60 text-muted-foreground hover:border-border hover:bg-foreground/[0.03]"
                    }`}
                  >
                    <Icon className={`h-4 w-4 ${active ? "text-primary" : ""}`} />
                    <span className="text-[11px] font-semibold">{p.label}</span>
                  </button>
                );
              })}
            </div>

            <div className="px-5 pt-2 pb-1 text-[12px] text-muted-foreground">
              {KINDS.find((p) => p.id === conn.kind)?.desc}
            </div>

            <div className="grid grid-cols-2 gap-3 px-5 pb-2 pt-2 max-h-[420px] overflow-y-auto scrollbar-mac">
              <Field label="Name" value={conn.name} onChange={(v) => update("name", v)} placeholder="Production Web" />

              {showS3 && (
                <>
                  <Field label="Region" value={conn.region ?? ""} onChange={(v) => update("region", v)} placeholder="auto" />
                  <div className="col-span-2">
                    <Field
                      label="Endpoint URL"
                      value={conn.endpoint ?? ""}
                      onChange={(v) => update("endpoint", v)}
                      placeholder={
                        conn.kind === "r2"
                          ? "https://<account>.r2.cloudflarestorage.com"
                          : conn.kind === "s3-compat"
                            ? "https://s3.example.com"
                            : "(leave empty for default AWS)"
                      }
                    />
                  </div>
                  <Field label="Default bucket" value={conn.bucket ?? ""} onChange={(v) => update("bucket", v)} placeholder="(optional)" />
                  <Field label="Default path" value={conn.defaultPath ?? ""} onChange={(v) => update("defaultPath", v)} placeholder="(optional)" />
                  <div className="col-span-2">
                    <Field
                      label="Access Key ID"
                      value={secret.accessKeyId ?? ""}
                      onChange={(v) => setSecret((s) => ({ ...s, accessKeyId: v }))}
                      placeholder="AKIA…"
                      icon={<KeyRound className="h-3.5 w-3.5" />}
                    />
                  </div>
                  <div className="col-span-2">
                    <Field
                      label="Secret Access Key"
                      value={secret.secretAccessKey ?? ""}
                      onChange={(v) => setSecret((s) => ({ ...s, secretAccessKey: v }))}
                      placeholder="••••••••"
                      type="password"
                      icon={<KeyRound className="h-3.5 w-3.5" />}
                    />
                  </div>
                  <label className="col-span-2 flex items-center gap-2 text-[12px] text-muted-foreground">
                    <input
                      type="checkbox"
                      className="h-3.5 w-3.5 rounded border-border accent-primary"
                      checked={conn.usePathStyle}
                      onChange={(e) => update("usePathStyle", e.target.checked)}
                    />
                    Force path-style addressing (needed for MinIO and some compat providers)
                  </label>
                </>
              )}

              {(showSsh || showFtp) && (
                <>
                  <Field label="Host" value={conn.host ?? ""} onChange={(v) => update("host", v)} placeholder="example.com" />
                  <Field
                    label="Port"
                    value={conn.port?.toString() ?? ""}
                    onChange={(v) => update("port", v ? Number(v) : null)}
                    placeholder={showSsh ? "22" : conn.kind === "ftps" && conn.ftpsImplicit ? "990" : "21"}
                  />
                  {showFtp && (
                    <>
                      <Select
                        label="Transfer mode"
                        value={conn.ftpMode ?? "passive"}
                        onChange={(v) => update("ftpMode", v as Connection["ftpMode"])}
                        options={[
                          { value: "passive", label: "Passive (PASV)" },
                          { value: "extended-passive", label: "Extended Passive (EPSV)" },
                          { value: "active", label: "Active (PORT)" },
                        ]}
                      />
                      {conn.kind === "ftps" && (
                        <Select
                          label="TLS negotiation"
                          value={conn.ftpsImplicit ? "implicit" : "explicit"}
                          onChange={(v) => update("ftpsImplicit", v === "implicit")}
                          options={[
                            { value: "explicit", label: "Explicit (AUTH TLS, port 21)" },
                            { value: "implicit", label: "Implicit (TLS on connect, port 990)" },
                          ]}
                        />
                      )}
                    </>
                  )}
                  <Field label="Username" value={conn.username ?? ""} onChange={(v) => update("username", v)} placeholder="deploy" />
                  {showFtp && (
                    <div className="col-span-2">
                      <Field
                        label="Password"
                        value={secret.password ?? ""}
                        onChange={(v) => setSecret((s) => ({ ...s, password: v }))}
                        placeholder="••••••••"
                        type="password"
                        icon={<KeyRound className="h-3.5 w-3.5" />}
                      />
                    </div>
                  )}
                  {showSsh && (
                    <>
                      <div className="col-span-2 flex flex-col gap-1">
                        <span className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
                          Authentication
                        </span>
                        <div className="flex gap-0.5 rounded-md border border-border/60 bg-card/60 p-0.5">
                          {([
                            ["password", "Password"],
                            ["key", "Private key"],
                          ] as const).map(([m, label]) => (
                            <button
                              key={m}
                              type="button"
                              onClick={() => chooseSshAuth(m)}
                              className={`flex-1 rounded px-2 py-1 text-[12px] font-medium transition ${
                                sshAuth === m
                                  ? "bg-primary text-primary-foreground shadow-sm"
                                  : "text-muted-foreground hover:text-foreground"
                              }`}
                            >
                              {label}
                            </button>
                          ))}
                        </div>
                      </div>

                      {sshAuth === "password" ? (
                        <div className="col-span-2">
                          <Field
                            label="Password"
                            value={secret.password ?? ""}
                            onChange={(v) => setSecret((s) => ({ ...s, password: v }))}
                            placeholder="••••••••"
                            type="password"
                            icon={<KeyRound className="h-3.5 w-3.5" />}
                          />
                        </div>
                      ) : (
                        <>
                          <div className="col-span-2">
                            <Field
                              label="Private key (PEM)"
                              value={secret.privateKeyPem ?? ""}
                              onChange={(v) => setSecret((s) => ({ ...s, privateKeyPem: v }))}
                              placeholder="-----BEGIN OPENSSH PRIVATE KEY-----…"
                              type="password"
                              icon={<KeyRound className="h-3.5 w-3.5" />}
                            />
                          </div>
                          <div className="col-span-2">
                            <Field
                              label="Key passphrase (optional)"
                              value={secret.passphrase ?? ""}
                              onChange={(v) => setSecret((s) => ({ ...s, passphrase: v }))}
                              placeholder="(if the key is encrypted)"
                              type="password"
                            />
                          </div>
                        </>
                      )}
                      <div className="col-span-2 rounded-md border border-border/60 bg-card/40 px-3 py-2 text-[11px] text-muted-foreground">
                        Host keys are accepted on first use, then remembered — a changed key is rejected.
                      </div>
                    </>
                  )}
                </>
              )}
            </div>

            {(error || testResult || !valid) && (
              <div className="px-5 pb-2 text-[12px]">
                {error && <div className="text-destructive">{error}</div>}
                {testResult && <div className="text-success">{testResult}</div>}
                {!valid && !error && (
                  <div className="text-muted-foreground">
                    Required to save: {problems.join(", ")}.
                  </div>
                )}
              </div>
            )}

            <div className="flex items-center justify-between border-t border-border/60 bg-background/30 px-5 py-3">
              <div className="text-[11px] text-muted-foreground">
                {KIND_LABEL[conn.kind]} · saved to OS keychain
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => save({ test: true })}
                  disabled={saving || !valid}
                  className="h-8 rounded-md border border-border/60 bg-card/60 px-3 text-[12px] font-medium text-foreground/80 hover:bg-foreground/5 disabled:opacity-50"
                >
                  Save & Test
                </button>
                <button
                  onClick={onClose}
                  disabled={saving}
                  className="h-8 rounded-md border border-border/60 bg-card/60 px-3 text-[12px] font-medium text-foreground/80 hover:bg-foreground/5 disabled:opacity-50"
                >
                  Cancel
                </button>
                <button
                  onClick={() => save({ test: false })}
                  disabled={saving || !valid}
                  className="h-8 rounded-md bg-primary px-3 text-[12px] font-semibold text-primary-foreground shadow-sm transition hover:brightness-110 active:scale-[0.98] disabled:opacity-50"
                >
                  {saving ? "Saving…" : "Save"}
                </button>
              </div>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function Select({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </span>
      <span className="flex h-8 items-center rounded-md border border-border/60 bg-card/60 px-1.5 transition focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/20">
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="w-full bg-transparent text-[13px] text-foreground outline-none"
        >
          {options.map((o) => (
            <option key={o.value} value={o.value} className="bg-popover text-foreground">
              {o.label}
            </option>
          ))}
        </select>
      </span>
    </label>
  );
}

function Field({
  label,
  placeholder,
  type = "text",
  icon,
  value,
  onChange,
}: {
  label: string;
  placeholder?: string;
  type?: string;
  icon?: React.ReactNode;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </span>
      <span className="flex h-8 items-center gap-2 rounded-md border border-border/60 bg-card/60 px-2.5 transition focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/20">
        {icon && <span className="text-muted-foreground">{icon}</span>}
        <input
          type={type}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          // Stop the webview from remembering values across connections and
          // offering / copying one connection's input into another's form.
          autoComplete={type === "password" ? "new-password" : "off"}
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          data-1p-ignore
          data-lpignore="true"
          className="w-full bg-transparent text-[13px] outline-none placeholder:text-muted-foreground/60"
        />
      </span>
    </label>
  );
}

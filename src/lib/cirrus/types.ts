export type ConnectionKind = "s3" | "r2" | "s3-compat" | "sftp" | "ftp" | "ftps";

export const KIND_LABEL: Record<ConnectionKind, string> = {
  s3: "S3",
  r2: "R2",
  "s3-compat": "S3-Compat",
  sftp: "SFTP",
  ftp: "FTP",
  ftps: "FTPS",
};

export const S3_KINDS: ConnectionKind[] = ["s3", "r2", "s3-compat"];
export const isS3Family = (k: ConnectionKind) => S3_KINDS.includes(k);

/** FTP/FTPS data-channel mode (mirrors the Rust `FtpMode`). */
export type FtpMode = "active" | "passive" | "extended-passive";

/** What a backend can do. Mirrors the Rust `Caps`; derived from `kind` server-side.
 *  Prefer keying UI behavior off these over checking the connection kind. */
export type Caps = {
  multipart: boolean;
  resume: boolean;
  virtualBuckets: boolean;
};

export type Connection = {
  id: string;
  name: string;
  kind: ConnectionKind;
  host?: string | null;
  port?: number | null;
  region?: string | null;
  endpoint?: string | null;
  bucket?: string | null;
  defaultPath?: string | null;
  username?: string | null;
  usePathStyle: boolean;
  ftpMode?: FtpMode | null;
  ftpsImplicit?: boolean;
  color: string;
  createdAt?: string | null;
  /** Capability profile from the backend. Present on connections returned by
   *  `list`/`upsert`; absent on unsaved drafts built in the UI. */
  caps?: Caps;
};

export type ConnectionSecret = {
  accessKeyId?: string | null;
  secretAccessKey?: string | null;
  sessionToken?: string | null;
  password?: string | null;
  privateKeyPem?: string | null;
  passphrase?: string | null;
};

export type LocalEntry = {
  name: string;
  path: string;
  kind: "folder" | "file";
  size?: number | null;
  modified?: string | null;
  hidden: boolean;
};

export type ObjectEntry = {
  key: string;
  name: string;
  kind: "folder" | "file";
  size?: number | null;
  modified?: string | null;
  etag?: string | null;
};

export type TransferEvent =
  | { type: "start"; id: string; name: string; total: number; direction: "up" | "down" }
  | { type: "progress"; id: string; transferred: number; total: number }
  | { type: "done"; id: string }
  | { type: "error"; id: string; error: AppError };

export type LogLevel = "debug" | "info" | "warn" | "error";

/** Structured error IR mirrored from the Rust `AppError`. */
export type ErrorCategory =
  | "auth"
  | "permission"
  | "not-found"
  | "network"
  | "tls"
  | "protocol"
  | "rate-limited"
  | "redirect"
  | "client"
  | "server"
  | "config"
  | "io"
  | "unknown";

export type StatusCode = { proto: "http" | "ftp" | "sftp" | "os"; value: number };

export type AppError = {
  category: ErrorCategory;
  connector?: string | null;
  phase?: string | null;
  code?: StatusCode | null;
  retryable: boolean;
  summary: string;
  detail?: string | null;
  remediation?: string | null;
};

export type LogEntry = {
  id: string;
  ts: number; // epoch millis
  level: LogLevel;
  scope: string; // "connection" | "transfer" | ...
  connector?: string | null;
  phase?: string | null; // "tls" | "passive" | "auth" | "transfer" | ...
  code?: StatusCode | null;
  category?: ErrorCategory | null; // set on error lines
  message: string;
  connection?: string | null;
  fields?: Record<string, unknown>;
};

/** A row in a file pane — the UI-facing shape both panes render. */
export type FileNode = {
  name: string;
  kind: "folder" | "file";
  size?: number;
  modified: string;
  ext?: string;
};

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export type Transfer = {
  id: string;
  name: string;
  direction: "up" | "down";
  total: number;
  transferred: number;
  status: "active" | "done" | "error";
  error?: AppError;
  startedAt: number;
};

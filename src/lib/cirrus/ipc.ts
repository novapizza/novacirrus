import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  Connection,
  ConnectionSecret,
  ObjectEntry,
  LocalEntry,
} from "./types";

export const api = {
  listConnections: () => invoke<Connection[]>("list_connections"),
  upsertConnection: (connection: Connection, secret?: ConnectionSecret) =>
    invoke<Connection>("upsert_connection", { connection, secret }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),
  testConnection: (id: string) => invoke<string>("test_connection", { id }),

  /** Open and pool a live session (explicit Connect). Rejects on auth/host-key/network failure. */
  connect: (id: string) => invoke<void>("connect", { id }),
  /** Close the pooled session (explicit Disconnect). Idempotent. */
  disconnect: (id: string) => invoke<void>("disconnect", { id }),
  /** Whether a connection currently has a live pooled session. */
  isConnected: (id: string) => invoke<boolean>("is_connected", { id }),

  remoteList: (connectionId: string, path: string) =>
    invoke<ObjectEntry[]>("remote_list", { connectionId, path }),
  remoteSearch: (connectionId: string, path: string, query: string) =>
    invoke<ObjectEntry[]>("remote_search", { connectionId, path, query }),
  remoteDownload: (connectionId: string, path: string, dest: string) =>
    invoke<string>("remote_download", { connectionId, path, dest }),
  remoteUpload: (connectionId: string, src: string, path: string) =>
    invoke<string>("remote_upload", { connectionId, src, path }),
  remoteDelete: (connectionId: string, path: string) =>
    invoke<void>("remote_delete", { connectionId, path }),

  fsHome: () => invoke<string>("fs_home"),
  fsList: (path: string, showHidden?: boolean) =>
    invoke<LocalEntry[]>("fs_list", { path, showHidden: !!showHidden }),
  fsParent: (path: string) => invoke<string>("fs_parent", { path }),
};

export const win = {
  close: () => getCurrentWindow().close(),
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  isMaximized: () => getCurrentWindow().isMaximized(),
  startDragging: () => getCurrentWindow().startDragging(),
};

/** Detect host platform for chrome layout (mac uses left traffic lights). */
export function getPlatform(): "mac" | "win" | "linux" {
  if (typeof navigator === "undefined") return "mac";
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("mac")) return "mac";
  if (ua.includes("win")) return "win";
  return "linux";
}

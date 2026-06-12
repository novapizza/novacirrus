import type { AppError, ErrorCategory } from "./types";

/**
 * Normalize anything thrown across the Tauri boundary into the `AppError` IR.
 *
 * Commands now reject with a structured `AppError` object, but invoke-layer
 * failures (and older paths) can still surface a plain string — this collapses
 * both into one shape so the UI never has to branch.
 */
export function appError(e: unknown): AppError {
  if (e && typeof e === "object" && "summary" in e) {
    return e as AppError;
  }
  const summary =
    typeof e === "string"
      ? e
      : e && typeof e === "object" && "message" in e
        ? String((e as { message: unknown }).message)
        : String(e);
  return { category: "unknown", retryable: false, summary };
}

/** Human one-liner: summary plus remediation hint when present. */
export function errText(e: unknown): string {
  const a = appError(e);
  return a.remediation ? `${a.summary} — ${a.remediation}` : a.summary;
}

export const CATEGORY_LABEL: Record<ErrorCategory, string> = {
  auth: "Auth",
  permission: "Permission",
  "not-found": "Not found",
  network: "Network",
  tls: "TLS",
  protocol: "Protocol",
  "rate-limited": "Rate limited",
  redirect: "Redirect",
  client: "Bad request",
  server: "Server error",
  config: "Config",
  io: "Local I/O",
  unknown: "Error",
};

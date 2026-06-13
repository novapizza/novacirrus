import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { LogEntry, LogLevel } from "./types";

const CAP = 1000; // ring buffer — drop oldest beyond this

let listenerPromise: Promise<UnlistenFn> | null = null;
const subscribers = new Set<(l: LogEntry[]) => void>();
let buffer: LogEntry[] = [];
let pending: LogEntry[] = []; // lines arrived since the last flush
let flushScheduled = false;

function emit() {
  for (const s of subscribers) s(buffer);
}

// Coalesce bursts into one state update per frame. The Debug-level protocol
// bridge (russh/suppaftp/rustls) emits hundreds of lines per second during a
// transfer; emitting per line re-filters and re-renders the whole (≤1000-row)
// list each time, saturating the main thread so Clear and the filters stop
// responding. Batching keeps the panel interactive.
function flush() {
  flushScheduled = false;
  if (pending.length === 0) return;
  buffer = [...buffer, ...pending];
  pending = [];
  if (buffer.length > CAP) buffer = buffer.slice(buffer.length - CAP);
  emit();
}

function scheduleFlush() {
  if (flushScheduled) return;
  flushScheduled = true;
  requestAnimationFrame(flush);
}

function ensureListener() {
  // Claim the guard synchronously: listen() is async, so checking a value
  // assigned only after `await` lets a second concurrent call (e.g. React
  // StrictMode's double-mount) register a duplicate listener before the
  // first resolves — which makes every log entry arrive twice.
  if (listenerPromise) return;
  listenerPromise = listen<LogEntry>("log", (e) => {
    pending.push(e.payload);
    scheduleFlush();
  });
}

/** Subscribe to the live log buffer (newest last). */
export function useLogs(): LogEntry[] {
  const [list, setList] = useState<LogEntry[]>(buffer);
  useEffect(() => {
    subscribers.add(setList);
    ensureListener();
    setList(buffer);
    return () => {
      subscribers.delete(setList);
    };
  }, []);
  return list;
}

export function clearLogs() {
  buffer = [];
  pending = []; // drop queued lines too, or the next flush would repopulate
  emit();
}

export const LEVEL_RANK: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { LogEntry, LogLevel } from "./types";

const CAP = 1000; // ring buffer — drop oldest beyond this

let unlisten: UnlistenFn | null = null;
const subscribers = new Set<(l: LogEntry[]) => void>();
let buffer: LogEntry[] = [];

function emit() {
  for (const s of subscribers) s(buffer);
}

async function ensureListener() {
  if (unlisten) return;
  unlisten = await listen<LogEntry>("log", (e) => {
    buffer = [...buffer, e.payload];
    if (buffer.length > CAP) buffer = buffer.slice(buffer.length - CAP);
    emit();
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
  emit();
}

export const LEVEL_RANK: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

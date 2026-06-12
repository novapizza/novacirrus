import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { Transfer, TransferEvent } from "./types";

let unlisten: UnlistenFn | null = null;
let subscribers = new Set<(t: Transfer[]) => void>();
let state: Map<string, Transfer> = new Map();

function emit() {
  const list = Array.from(state.values()).sort((a, b) => b.startedAt - a.startedAt);
  for (const s of subscribers) s(list);
}

async function ensureListener() {
  if (unlisten) return;
  unlisten = await listen<TransferEvent>("transfer", (e) => {
    const p = e.payload;
    const now = Date.now();
    const cur = state.get(p.id);
    switch (p.type) {
      case "start":
        state.set(p.id, {
          id: p.id,
          name: p.name,
          direction: p.direction,
          total: p.total,
          transferred: 0,
          status: "active",
          startedAt: now,
        });
        break;
      case "progress":
        if (cur) state.set(p.id, { ...cur, transferred: p.transferred, total: p.total || cur.total });
        break;
      case "done":
        if (cur) state.set(p.id, { ...cur, status: "done", transferred: cur.total });
        break;
      case "error":
        if (cur) state.set(p.id, { ...cur, status: "error", error: p.error });
        break;
    }
    emit();
  });
}

export function useTransfers(): Transfer[] {
  const [list, setList] = useState<Transfer[]>([]);
  useEffect(() => {
    subscribers.add(setList);
    ensureListener();
    setList(Array.from(state.values()).sort((a, b) => b.startedAt - a.startedAt));
    return () => {
      subscribers.delete(setList);
    };
  }, []);
  return list;
}

export function clearDoneTransfers() {
  for (const [id, t] of state) {
    if (t.status === "done" || t.status === "error") state.delete(id);
  }
  emit();
}

export function dismissTransfer(id: string) {
  const t = state.get(id);
  if (!t || t.status === "active") return;
  state.delete(id);
  emit();
}

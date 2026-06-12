import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/cirrus/ipc";
import type { Connection, ConnectionSecret } from "@/lib/cirrus/types";
import { errText } from "@/lib/cirrus/errors";

export function useConnections() {
  const [items, setItems] = useState<Connection[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      const list = await api.listConnections();
      setItems(list);
    } catch (e) {
      setError(errText(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const save = useCallback(
    async (c: Connection, secret?: ConnectionSecret) => {
      const saved = await api.upsertConnection(c, secret);
      await refresh();
      return saved;
    },
    [refresh],
  );

  const remove = useCallback(
    async (id: string) => {
      await api.deleteConnection(id);
      await refresh();
    },
    [refresh],
  );

  return { items, loading, error, refresh, save, remove };
}

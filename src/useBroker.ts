import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { BrokerState } from "./types";

export function useBroker(onChanged: () => Promise<unknown>) {
  const [broker, setBroker] = useState<BrokerState | null>(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const epoch = useRef(0);
  useEffect(() => {
    let live = true;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      const e = epoch.current;
      try {
        const b = await api.broker();
        if (live && e === epoch.current) setBroker(b);
      } catch (e) {
        if (live) setError(String(e));
      } finally {
        pending = false;
      }
    };
    void refresh();
    const id = window.setInterval(refresh, 1200);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, []);
  const perform = async (label: string, action: () => Promise<unknown>) => {
    epoch.current++;
    setBusy(label);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      epoch.current++;
      try {
        setBroker(await api.broker());
        await onChanged();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy("");
      }
    }
  };
  return { broker, busy, error, setError, perform };
}

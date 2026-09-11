import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type { RemoteState } from "./types";

export function useRemoteAccess() {
  const [state, setState] = useState<RemoteState | null>(null);
  const [error, setError] = useState("");
  const [readError, setReadError] = useState("");
  const [busy, setBusy] = useState("");
  const [approvalBusy, setApprovalBusy] = useState(false);
  const [approvalError, setApprovalError] = useState("");
  const approving = useRef(false);
  const epoch = useRef(0);
  const live = useRef(false);
  const mutation = useRef(false);
  const refresh = useCallback(async () => {
    const current = ++epoch.current;
    try {
      const next = await api.remoteState();
      if (live.current && current === epoch.current) { setState(next); setReadError(""); }
    } catch (e) {
      if (live.current && current === epoch.current) { setReadError(`无法读取远程状态：${String(e)}`); }
    }
  }, []);
  useEffect(() => {
    live.current = true;
    let disposed = false;
    let pending = false;
    const poll = async () => {
      if (pending) return;
      pending = true;
      try { await refresh(); } finally { pending = false; }
    };
    void poll();
    const timer = window.setInterval(poll, 1000);
    let unlisten: (() => void) | undefined;
    void listen("remote-authorization", () => { void refresh(); })
      .then(stop => { if (disposed) stop(); else unlisten = stop; })
      .catch(() => { /* Status polling still delivers approvals if events are unavailable. */ });
    return () => { disposed = true; live.current = false; window.clearInterval(timer); unlisten?.(); };
  }, [refresh]);
  async function operate(label: string, action: () => Promise<void>) {
    if (mutation.current) return false;
    mutation.current = true;
    epoch.current++;
    setBusy(label);
    setError("");
    try {
      await action();
      if (live.current) await refresh();
      return true;
    } catch (e) {
      if (live.current) { setError(String(e)); await refresh(); }
      return false;
    } finally {
      mutation.current = false;
      if (live.current) setBusy("");
    }
  }
  async function approve(id: string, allow: boolean) {
    if (approving.current) return false;
    approving.current = true;
    epoch.current++;
    setApprovalBusy(true);
    setApprovalError("");
    try {
      await api.remoteApprove(id, allow);
      if (live.current) await refresh();
      return true;
    } catch (e) {
      if (live.current) { setApprovalError(String(e)); await refresh(); }
      return false;
    } finally {
      approving.current = false;
      if (live.current) setApprovalBusy(false);
    }
  }
  return { state, error: readError || error, busy, operate, approvalBusy, approvalError, approve };
}
export type RemoteController = ReturnType<typeof useRemoteAccess>;

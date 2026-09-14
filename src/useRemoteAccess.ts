import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { api } from "./api";
import type { RemoteState } from "./types";

const quickTunnelProgressStates = new Set<RemoteState["status"]>(["starting", "installing", "discovering_url", "verifying"]);

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
  const quickTunnelBaseline = useRef<Pick<RemoteState, "mode" | "status" | "lastError"> | null>(null);
  const quickTunnelAttempt = useRef(0);
  const pendingQuickTunnelAttempt = useRef<number | null>(null);
  const pendingQuickTunnelAttemptError = useRef<string | null>(null);
  const quickTunnelCommandPending = useRef(false);
  const [quickTunnelCommandPendingState, setQuickTunnelCommandPendingState] = useState(false);
  const deferredQuickTunnelFailure = useRef<RemoteState | null>(null);
  const [quickTunnelHandledError, setQuickTunnelHandledError] = useState<string | null>(null);
  function notifyQuickTunnelFailure(attempt: number, error: string | null, message: string) {
    if (error) setQuickTunnelHandledError(error);
    toast.error(message, { id: `quick-tunnel-start-${attempt}` });
  }
  const observeQuickTunnel = useCallback((next: RemoteState) => {
    const previous = quickTunnelBaseline.current;
    quickTunnelBaseline.current = next;
    if (!previous) return;
    const terminalFailure = next.mode === "quick_tunnel" && (next.status === "error" || next.status === "disconnected");
    const attempt = pendingQuickTunnelAttempt.current;
    const newAttemptError = !!next.lastError && next.lastError !== pendingQuickTunnelAttemptError.current;
    if (attempt !== null && (terminalFailure || newAttemptError)) {
      if (quickTunnelCommandPending.current) {
        deferredQuickTunnelFailure.current = next;
      } else {
        pendingQuickTunnelAttempt.current = null;
        pendingQuickTunnelAttemptError.current = null;
        notifyQuickTunnelFailure(attempt, next.lastError, "快捷隧道启动失败，请重新启动。");
      }
      return;
    }
    if (attempt !== null && (
      (next.mode === "quick_tunnel" && (next.status === "ready" || next.status === "stopped"))
      || (previous.mode === "quick_tunnel" && next.mode !== "quick_tunnel")
    )) {
      pendingQuickTunnelAttempt.current = null;
      deferredQuickTunnelFailure.current = null;
      pendingQuickTunnelAttemptError.current = null;
    }
    if (attempt === null
      && previous.mode === "quick_tunnel"
      && quickTunnelProgressStates.has(previous.status)
      && terminalFailure) {
      toast.error("快捷隧道连接失败，请重新启动。");
    }
  }, []);
  const refresh = useCallback(async () => {
    const current = ++epoch.current;
    try {
      const next = await api.remoteState();
      if (live.current && current === epoch.current) { observeQuickTunnel(next); setState(next); setReadError(""); }
    } catch (e) {
      if (live.current && current === epoch.current) { setReadError(`无法读取远程状态：${String(e)}`); }
    }
  }, [observeQuickTunnel]);
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
  async function startQuickTunnel() {
    const attempt = ++quickTunnelAttempt.current;
    pendingQuickTunnelAttempt.current = attempt;
    pendingQuickTunnelAttemptError.current = quickTunnelBaseline.current?.lastError ?? null;
    deferredQuickTunnelFailure.current = null;
    quickTunnelCommandPending.current = true;
    setQuickTunnelCommandPendingState(true);
    const started = await operate("开启", () => api.remoteStart("quick_tunnel"));
    quickTunnelCommandPending.current = false;
    if (live.current) setQuickTunnelCommandPendingState(false);
    if (!started) {
      pendingQuickTunnelAttempt.current = null;
      deferredQuickTunnelFailure.current = null;
      pendingQuickTunnelAttemptError.current = null;
      notifyQuickTunnelFailure(attempt, null, "快捷隧道启动失败，请检查状态后重试。");
    } else {
      const failure = deferredQuickTunnelFailure.current as RemoteState | null;
      if (!failure) return started;
      pendingQuickTunnelAttempt.current = null;
      pendingQuickTunnelAttemptError.current = null;
      deferredQuickTunnelFailure.current = null;
      notifyQuickTunnelFailure(attempt, failure.lastError, "快捷隧道启动失败，请重新启动。");
    }
    return started;
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
  return { state, error: readError || error, busy, operate, refresh, startQuickTunnel, quickTunnelCommandPending: quickTunnelCommandPendingState, quickTunnelHandledError, approvalBusy, approvalError, approve };
}
export type RemoteController = ReturnType<typeof useRemoteAccess>;

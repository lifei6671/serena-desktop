import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { api } from "./api";
import type { BrokerState } from "./types";

export function useBroker(onChanged: () => Promise<unknown>) {
  const [broker, setBroker] = useState<BrokerState | null>(null);
  const [busy, setBusy] = useState("");
  const epoch = useRef(0);
  useEffect(() => {
    let live = true;
    let pending = false;
    let lastReadError = "";
    const refresh = async () => {
      if (pending) return;
      pending = true;
      const e = epoch.current;
      try {
        const b = await api.broker();
        if (live && e === epoch.current) {
          setBroker(b);
          lastReadError = "";
        }
      } catch (reason) {
        if (live && e === epoch.current && lastReadError !== String(reason)) {
          lastReadError = String(reason);
          toast.error(`工作区状态读取失败：${lastReadError}`, {
            id: "broker-read-error",
          });
        }
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
  const backendError = broker?.lastError;
  const warnings = broker?.syncWarnings.join("\n");
  useEffect(() => {
    if (backendError) toast.error(backendError, { id: "broker-feedback" });
  }, [backendError]);
  useEffect(() => {
    if (warnings)
      toast.warning("项目同步存在提示", {
        description: warnings,
        id: "sync-warnings",
      });
  }, [warnings]);
  const perform = async (
    label: string,
    action: () => Promise<unknown>,
    success?: string,
  ) => {
    epoch.current++;
    setBusy(label);
    try {
      await action();
      if (success) toast.success(success, { id: "broker-feedback" });
    } catch (reason) {
      toast.error(String(reason), { id: "broker-feedback" });
    } finally {
      epoch.current++;
      try {
        setBroker(await api.broker());
        await onChanged();
      } catch (reason) {
        toast.error(`状态刷新失败：${String(reason)}`, {
          id: "broker-read-error",
        });
      } finally {
        setBusy("");
      }
    }
  };
  return { broker, busy, perform };
}

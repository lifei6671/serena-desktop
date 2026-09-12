import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useBroker } from "../useBroker";
import type { AppState, ManagerConfig } from "../types";

const initialConfig: ManagerConfig = {
  agentEnabled: false,
  remoteAccess: { mode: "mcp_only", selfHosted: { provider: "custom_https", publicOrigin: null }, mcpOnly: { securityDeclaration: "external_auth", publicOrigin: null } },
  broker: { enabled: false, port: 9120, allowLan: false },
  workspaces: [],
  serenaPath: null,
  port: 9121,
  dashboardEnabled: true,
  openDashboardOnLaunch: false,
  autoStartServer: true,
  minimizeToTray: true,
};

export function useAppController(statusVisible: boolean) {
  const [state, setState] = useState<AppState | null>(null);
  const [draft, setDraft] = useState<ManagerConfig>(initialConfig);
  const [busy, setBusy] = useState<string | null>(null);
  const [brokerPort, setBrokerPort] = useState(9120);
  const [brokerAllowLan, setBrokerAllowLan] = useState(false);
  const hydrated = useRef(false);
  const requestEpoch = useRef(0);
  const mutationActive = useRef(false);
  const pickerActive = useRef(false);
  const [choosingExecutable, setChoosingExecutable] = useState(false);
  const [codexVersion, setCodexVersion] = useState("");
  const [codexError, setCodexError] = useState("");
  const [codexLoading, setCodexLoading] = useState(false);
  const codexDetection = useRef<Promise<void> | null>(null);
  const detectCodex = useCallback((force = false) => {
    if (codexDetection.current && !force) return codexDetection.current;
    setCodexLoading(true);
    codexDetection.current = api.codexVersion()
      .then(version => { setCodexVersion(version); setCodexError(""); })
      .catch(error => { setCodexVersion(""); setCodexError(String(error)); })
      .finally(() => setCodexLoading(false));
    return codexDetection.current;
  }, []);
  useEffect(() => {
    if (statusVisible) void detectCodex();
  }, [statusVisible, detectCodex]);

  const applySnapshot = useCallback((next: AppState) => {
    if (!hydrated.current) {
      hydrated.current = true;
      setDraft(next.config);
      setBrokerPort(next.config.broker.port);
      setBrokerAllowLan(next.config.broker.allowLan);
    }
    setState(next);
  }, []);

  const refresh = async () => {
    const next = await api.getState();
    applySnapshot(next);
    return next;
  };

  const brokerController = useBroker(refresh);
  const updatingBroker = brokerController.busy === "更新连接入口";

  useEffect(() => {
    let active = true;
    let lastReadError = "";
    const reportReadError = (reason: unknown) => {
      if (active && lastReadError !== String(reason)) {
        lastReadError = String(reason);
        toast.error(`服务状态读取失败：${lastReadError}`, {
          id: "app-read-error",
        });
      }
    };
    api
      .getState()
      .then((next) => {
        if (!active) return;
        applySnapshot(next);
        lastReadError = "";
      })
      .catch(reportReadError);

    const timer = window.setInterval(() => {
      if (!active || mutationActive.current) return;
      const epoch = requestEpoch.current;
      api
        .getState()
        .then((next) => {
          if (active && epoch === requestEpoch.current) {
            applySnapshot(next);
            lastReadError = "";
          }
        })
        .catch((reason: unknown) => {
          if (epoch === requestEpoch.current) reportReadError(reason);
        });
    }, 1500);

    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [applySnapshot]);

  const run = async (
    label: string,
    action: () => Promise<AppState>,
    success?: string,
  ) => {
    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy(label);
    try {
      const next = await action();
      setState(next);
      if (success) toast.success(success);
    } catch (reason) {
      toast.error(String(reason), { id: "app-feedback" });
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const saveFields = async (patch: Partial<ManagerConfig>, success: string) => {
    if (!state || busy !== null) return;
    const unchanged = Object.entries(patch).every(
      ([key, value]) => state.config[key as keyof ManagerConfig] === value,
    );
    if (unchanged) return;

    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy("field");
    try {
      const snapshot = await api.saveConfig({ ...state.config, ...patch });
      setState(snapshot);
      setDraft((current) => ({ ...current, ...patch }));
      toast.success(success);
    } catch (reason) {
      toast.error(String(reason), { id: "app-feedback" });
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const saveToggle = async (patch: Partial<ManagerConfig>, success: string) => {
    if (!state) return;
    const previous = draft;
    const optimistic = { ...draft, ...patch };
    const persisted = { ...state.config, ...patch };
    setDraft(optimistic);
    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy("toggle");
    try {
      const snapshot = await api.saveConfig(persisted);
      setState(snapshot);
      setDraft((current) => ({ ...current, ...patch }));
      toast.success(success);
    } catch (reason) {
      setDraft(previous);
      toast.error(String(reason), { id: "app-feedback" });
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const runSideEffect = async (label: string, action: () => Promise<void>) => {
    setBusy(label);
    try {
      await action();
    } catch (reason) {
      toast.error(String(reason), { id: "app-feedback" });
    } finally {
      setBusy(null);
    }
  };

  const setAutostart = async (enabled: boolean) => {
    const previous = state?.autostartEnabled ?? null;
    setState((current) =>
      current ? { ...current, autostartEnabled: enabled } : current,
    );
    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy("autostart");
    try {
      const snapshot = await api.setAutostart(enabled);
      setState(snapshot);
      toast.success(
        enabled ? "已启用 Windows 登录自启。" : "已关闭 Windows 登录自启。",
      );
    } catch (reason) {
      setState((current) =>
        current ? { ...current, autostartEnabled: previous } : current,
      );
      toast.error(String(reason), { id: "app-feedback" });
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const chooseSerenaExecutable = async () => {
    pickerActive.current = true;
    setChoosingExecutable(true);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        defaultPath: draft.serenaPath ?? undefined,
        filters: [{ name: "Serena executable", extensions: ["exe"] }],
      });
      if (typeof selected === "string") {
        setDraft((current) => ({ ...current, serenaPath: selected }));
        await saveFields({ serenaPath: selected }, "Serena 可执行文件已保存。");
      } else {
        await saveFields(
          { serenaPath: draft.serenaPath },
          "Serena 可执行文件已保存。",
        );
      }
    } catch (reason) {
      toast.error(String(reason), { id: "app-feedback" });
    } finally {
      pickerActive.current = false;
      setChoosingExecutable(false);
    }
  };

  return { state, draft, setDraft, busy, brokerPort, setBrokerPort, brokerAllowLan, setBrokerAllowLan, pickerActive, choosingExecutable, codexVersion, codexError, codexLoading, detectCodex, brokerController, updatingBroker, run, saveFields, saveToggle, runSideEffect, setAutostart, chooseSerenaExecutable };
}
export type AppController = ReturnType<typeof useAppController>;

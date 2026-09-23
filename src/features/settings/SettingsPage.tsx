import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { api } from "../../api";
import type { AppState } from "../../types";
import type { AppController } from "../../app/useAppController";
import { Check, Copy, PlayCircle, StopCircle, Terminal } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { toast } from "sonner";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Field, FieldGroup, FieldContent, FieldLabel, FieldDescription } from "@/components/ui/field";
function SettingSwitch({
  checked,
  onChange,
  label,
  hint,
  disabled = false,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  hint: string;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <Field className="settings-switch" orientation="horizontal" data-disabled={disabled}>
      <FieldContent>
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        <FieldDescription id={`${id}-description`}>{hint}</FieldDescription>
      </FieldContent>
      <Switch
        id={id}
        aria-describedby={`${id}-description`}
        checked={checked}
        onCheckedChange={onChange}
        disabled={disabled}
      />
    </Field>
  );
}

export default function SettingsPage({ state, draft, setDraft, busy, brokerPort, setBrokerPort, brokerAllowLan, setBrokerAllowLan, pickerActive, choosingExecutable, brokerController, updatingBroker, saveFields, saveToggle, setAutostart, chooseSerenaExecutable }: { state: AppState } & Pick<AppController, "draft" | "setDraft" | "busy" | "brokerPort" | "setBrokerPort" | "brokerAllowLan" | "setBrokerAllowLan" | "pickerActive" | "choosingExecutable" | "brokerController" | "updatingBroker" | "saveFields" | "saveToggle" | "setAutostart" | "chooseSerenaExecutable">) {
  const [copiedDetectedPath, setCopiedDetectedPath] = useState(false);
  const copyResetTimer = useRef<number | null>(null);
  const detectedPath = state.installation?.path ?? null;
  const brokerRunning = !!brokerController.broker?.running;

  useEffect(() => () => {
    if (copyResetTimer.current !== null) window.clearTimeout(copyResetTimer.current);
  }, []);

  const copyDetectedPath = async () => {
    if (!detectedPath || copiedDetectedPath) return;
    try {
      if (!navigator.clipboard?.writeText) throw new Error("剪贴板不可用");
      await navigator.clipboard.writeText(detectedPath);
      setCopiedDetectedPath(true);
      if (copyResetTimer.current !== null) window.clearTimeout(copyResetTimer.current);
      copyResetTimer.current = window.setTimeout(() => {
        copyResetTimer.current = null;
        setCopiedDetectedPath(false);
      }, 1500);
    } catch (error) {
      toast.error(`复制当前检测路径失败：${String(error)}`);
    }
  };

  return (
          <section className="settings-page">
            <div className="page-heading">
              <div>
                <h1>设置</h1>
                <p>开关与文件选择即时生效；输入项在离开时自动保存。</p>
              </div>
              <span className="settings-autosave-status"><i aria-hidden="true" />自动持久化就绪</span>
            </div>

            <div className="settings-section">
              <header>
                <span>01</span>
                <div>
                  <h2>General</h2>
                  <p>系统与应用生命周期</p>
                </div>
              </header>
              <FieldGroup className="settings-body">
                <SettingSwitch
                  checked={draft.agentEnabled}
                  onChange={value => saveToggle({ agentEnabled: value }, value ? "已开启 Agent 对外工具。" : "已关闭 Agent 对外工具。")}
                  disabled={busy !== null}
                  label="开启 Agent"
                  hint="允许 MCP 客户端使用 Agent 工具。关闭后拒绝新的工具请求；不取消已有任务，桌面端仍可管理任务。客户端需刷新工具列表。"
                />
                <SettingSwitch
                  checked={draft.remoteSourceWriteEnabled}
                  onChange={value => saveToggle(
                    { remoteSourceWriteEnabled: value },
                    value ? "已允许远程修改项目文件；重新连接 MCP 客户端后生效。" : "已关闭远程项目文件修改。",
                  )}
                  disabled={busy !== null}
                  label="允许远程修改项目文件"
                  hint={draft.remoteSourceWriteEnabled
                    ? "允许远程 MCP 客户端使用文件创建、写入、插入、删除和替换工具。重新连接 MCP 客户端后生效。"
                    : "远程 MCP 客户端只能读取项目文件。"}
                />
                <SettingSwitch
                  checked={state.autostartEnabled ?? false}
                  onChange={setAutostart}
                  disabled={busy !== null || state.autostartEnabled === null}
                  label="随系统登录启动"
                  hint={
                    state.autostartError ?? "由系统登录项启动 Serena Desktop"
                  }
                />
                <SettingSwitch
                  checked={draft.autoStartServer}
                  onChange={(value) =>
                    saveToggle(
                      { autoStartServer: value },
                      value
                        ? "已启用自动启动 Serena。"
                        : "已关闭自动启动 Serena。",
                    )
                  }
                  disabled={busy !== null}
                  label="自动启动 Serena"
                  hint="应用启动后自动启动 Serena 代码分析服务"
                />
                <SettingSwitch
                  checked={draft.minimizeToTray}
                  onChange={(value) =>
                    saveToggle(
                      { minimizeToTray: value },
                      value
                        ? "关闭窗口后应用将继续在后台运行。"
                        : "关闭窗口时将退出应用。",
                    )
                  }
                  disabled={busy !== null}
                  label="关闭窗口时保留后台运行"
                  hint="通过 Serena Desktop 常驻菜单中的“退出”可结束应用"
                />
              </FieldGroup>
            </div>

            <div className="settings-section">
              <header>
                <span>02</span>
                <div>
                  <h2>Agent 提醒</h2>
                  <p>任务业务终态的桌面反馈</p>
                </div>
              </header>
              <FieldGroup className="settings-body">
                <SettingSwitch
                  checked={draft.agentSuccessNotificationEnabled}
                  onChange={value => saveToggle({ agentSuccessNotificationEnabled: value }, value ? "已开启 Agent 任务成功提醒。" : "已关闭 Agent 任务成功提醒。")}
                  disabled={busy !== null}
                  label="任务成功提醒"
                  hint="Agent 任务完成后触发提醒。"
                />
                <SettingSwitch
                  checked={draft.agentFailureNotificationEnabled}
                  onChange={value => saveToggle({ agentFailureNotificationEnabled: value }, value ? "已开启 Agent 任务异常提醒。" : "已关闭 Agent 任务异常提醒。")}
                  disabled={busy !== null}
                  label="任务异常提醒"
                  hint="Agent 任务失败或中断后触发提醒；用户主动取消不会提醒。"
                />
                <SettingSwitch
                  checked={draft.agentSystemNotificationEnabled}
                  onChange={value => saveToggle({ agentSystemNotificationEnabled: value }, value ? "已开启系统通知。" : "已关闭系统通知。")}
                  disabled={busy !== null}
                  label="系统通知"
                  hint="通过操作系统显示任务提醒。"
                />
                <SettingSwitch
                  checked={draft.agentSoundEnabled}
                  onChange={value => saveToggle({ agentSoundEnabled: value }, value ? "已开启提示音。" : "已关闭提示音。")}
                  disabled={busy !== null}
                  label="提示音"
                  hint="任务提醒发生时播放系统提示音。"
                />
              </FieldGroup>
            </div>

            <div className="settings-section">
              <header>
                <span>03</span>
                <div>
                  <h2>Serena</h2>
                  <p>可执行文件发现</p>
                </div>
              </header>
              <FieldGroup className="settings-body field-stack">
                <Field className="text-field">
                  <FieldLabel htmlFor="serena-executable">
                    Executable
                  </FieldLabel>
                  <div className="input-action-row">
                    <Input
                      id="serena-executable"
                      disabled={busy !== null}
                      value={draft.serenaPath ?? ""}
                      onChange={(event) =>
                        setDraft({
                          ...draft,
                          serenaPath: event.target.value || null,
                        })
                      }
                      onBlur={(event) => {
                        if (
                          event.relatedTarget instanceof HTMLElement &&
                          event.relatedTarget.dataset.executablePicker ===
                            "true"
                        )
                          return;
                        saveFields(
                          { serenaPath: draft.serenaPath },
                          "Serena 可执行文件已保存。",
                        );
                      }}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") event.currentTarget.blur();
                      }}
                      placeholder="使用 Managed 官方 Serena"
                      spellCheck={false}
                    />
                    <Button
                      type="button"
                      className="settings-executable-picker"
                      variant="outline"
                      data-executable-picker="true"
                      aria-busy={choosingExecutable}
                      disabled={busy !== null || choosingExecutable}
                      onClick={chooseSerenaExecutable}
                      onBlur={() => {
                        if (!pickerActive.current)
                          saveFields(
                            { serenaPath: draft.serenaPath },
                            "Serena 可执行文件已保存。",
                          );
                      }}
                    >
                      <span className="settings-button-icon" data-slot="settings-executable-picker-icon">
                        {choosingExecutable && <Spinner aria-hidden="true" />}
                      </span>
                      <span>选择…</span>
                    </Button>
                  </div>
                  <FieldDescription>
                    留空时优先使用应用私有 runtime；未安装时检测
                    PATH。指定外部路径必须为受支持的官方 Serena 1.7.0 或以上。
                  </FieldDescription>
                </Field>
                <div className="detected-path">
                  <span className="detected-path-label">当前检测</span>
                  <div className="detected-path-value">
                    <Terminal aria-hidden="true" />
                    <code>{detectedPath ?? "未发现"}</code>
                    {detectedPath && (
                      <button
                        type="button"
                        className="detected-path-copy"
                        data-copied={copiedDetectedPath}
                        aria-label={copiedDetectedPath ? "当前检测路径已复制" : "复制当前检测路径"}
                        onClick={copyDetectedPath}
                      >
                        <span className="settings-button-icon" aria-hidden="true">
                          {copiedDetectedPath ? <Check /> : <Copy />}
                        </span>
                        <span>{copiedDetectedPath ? "已复制" : "复制"}</span>
                      </button>
                    )}
                  </div>
                </div>
              </FieldGroup>
            </div>

            <div className="settings-section">
              <header>
                <span>04</span>
                <div>
                  <h2>Serena 内部服务</h2>
                  <p>提供代码分析能力，由 MCP 连接入口调用</p>
                </div>
              </header>
              <FieldGroup className="settings-body field-stack">
                <Field className="settings-port-field">
                  <FieldLabel htmlFor="serena-port">Serena 内部服务端口</FieldLabel>
                  <Input
                    disabled={busy !== null}
                    type="number"
                    min={1024}
                    max={65535}
                    id="serena-port"
                    value={draft.port}
                    onChange={(event) =>
                      setDraft({ ...draft, port: Number(event.target.value) })
                    }
                    onBlur={() =>
                      saveFields(
                        { port: draft.port },
                        "Serena 内部端口已保存；重新启动 Serena 后生效。",
                      )
                    }
                    onKeyDown={(event) => {
                      if (event.key === "Enter") event.currentTarget.blur();
                    }}
                  />
                  <FieldDescription>
                    Serena Desktop 仅在本机使用此端口提供内部 Serena 服务，与下方 MCP Broker 连接入口端口彼此独立；两者不能相同。允许范围
                    1024–65535；变更后需重新启动 Serena。
                  </FieldDescription>
                </Field>
                <SettingSwitch
                  checked={draft.dashboardEnabled}
                  onChange={(value) =>
                    saveToggle(
                      {
                        dashboardEnabled: value,
                        openDashboardOnLaunch:
                          value && state.config.openDashboardOnLaunch,
                      },
                      value
                        ? "已保存：启用管理面板，重启 Serena 后生效。"
                        : "已保存：关闭管理面板，重启 Serena 后生效。",
                    )
                  }
                  disabled={busy !== null}
                  label="启用浏览器管理面板"
                  hint="使用浏览器查看 Serena 运行信息；更改后需重新启动 Serena"
                />
                <SettingSwitch
                  checked={draft.openDashboardOnLaunch}
                  onChange={(value) =>
                    saveToggle(
                      { openDashboardOnLaunch: value },
                      value
                        ? "启动 Serena 时将自动打开 Dashboard。"
                        : "已关闭 Dashboard 自动打开。",
                    )
                  }
                  disabled={!draft.dashboardEnabled || busy !== null}
                  label="启动时在浏览器打开管理面板"
                  hint="默认关闭；也可从 Serena 页面手工打开"
                />
              </FieldGroup>
            </div>

            <div className="settings-section">
              <header>
                <div>
                  <h2>MCP 连接入口</h2>
                  <p>汇集 Serena 代码分析与 Git 查询能力</p>
                </div>
              </header>
              <FieldGroup className="settings-body field-stack">
                <Field className="settings-port-field">
                  <FieldLabel htmlFor="broker-port">MCP Broker 连接入口端口</FieldLabel>
                  <Input
                    type="number"
                    min={1024}
                    max={65535}
                    id="broker-port"
                    value={brokerPort}
                    disabled={
                      !!brokerController.busy ||
                      brokerRunning
                    }
                    onChange={(e) => setBrokerPort(Number(e.target.value))}
                  />
                  <FieldDescription>
                    ChatGPT/Cloudflare/Claude 等客户端连接的主入口，不是 Serena 内部端口。本机地址可在首页复制。停止入口后可修改端口和访问范围，重新启用时生效。
                  </FieldDescription>
                </Field>
                <SettingSwitch
                  label="允许局域网连接"
                  checked={brokerAllowLan}
                  onChange={setBrokerAllowLan}
                  disabled={!!brokerController.busy || !!brokerController.broker?.running}
                  hint={brokerAllowLan
                    ? `开启后监听所有 IPv4 网卡（0.0.0.0）。其他电脑使用 http://本机局域网IPv4地址:${brokerPort}/mcp。服务无内置认证，能访问端口的设备可读取和切换共享工作区，请仅在可信网络启用；如被防火墙拦截，需手动允许对应端口。`
                    : "关闭时仅本机可连接（127.0.0.1），其他电脑无法访问。"}
                />
                <div>
                  <Button
                    className="settings-broker-action"
                    variant="outline"
                    disabled={
                      busy !== null ||
                      !!brokerController.busy ||
                      !brokerController.broker
                    }
                    onClick={() =>
                      brokerController.perform(
                        "更新连接入口",
                        () =>
                          api.setBroker(
                            !brokerController.broker?.running,
                            brokerPort,
                            brokerAllowLan,
                          ),
                        brokerRunning
                          ? "MCP 连接入口已停止"
                          : "MCP 连接入口已启用",
                      )
                    }
                    aria-busy={updatingBroker}
                    data-running={brokerRunning}
                  >
                    <span className="settings-button-icon" data-slot="settings-broker-action-icon">
                      {updatingBroker ? <Spinner aria-hidden="true" /> : brokerRunning ? <StopCircle aria-hidden="true" /> : <PlayCircle aria-hidden="true" />}
                    </span>
                    <span data-slot="settings-broker-action-label">
                      {updatingBroker ? "处理中…" : brokerRunning ? "停止连接入口" : "启用连接入口"}
                    </span>
                  </Button>
                </div>
              </FieldGroup>
            </div>
          </section>
  );
}

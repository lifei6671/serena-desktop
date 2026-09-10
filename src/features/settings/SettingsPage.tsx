import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { api } from "../../api";
import type { AppState } from "../../types";
import type { AppController } from "../../app/useAppController";
import { useId } from "react";
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
    <Field orientation="horizontal" data-disabled={disabled}>
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
  return (
          <section className="settings-page">
            <div className="page-heading">
              <div>
                <h1>设置</h1>
                <p>开关与文件选择即时生效；输入项在离开时自动保存。</p>
              </div>
            </div>

            <div className="settings-section">
              <header>
                <span>01</span>
                <div>
                  <h2>General</h2>
                  <p>Windows 与应用生命周期</p>
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
                  checked={state.autostartEnabled ?? false}
                  onChange={setAutostart}
                  disabled={busy !== null || state.autostartEnabled === null}
                  label="Windows 登录后启动"
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
                        ? "关闭窗口时将进入托盘。"
                        : "关闭窗口时将退出应用。",
                    )
                  }
                  disabled={busy !== null}
                  label="关闭窗口时进入托盘"
                  hint="只有托盘菜单中的“退出”会结束应用"
                />
              </FieldGroup>
            </div>

            <div className="settings-section">
              <header>
                <span>02</span>
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
                      {choosingExecutable && (
                        <Spinner data-icon="inline-start" aria-hidden="true" />
                      )}
                      选择…
                    </Button>
                  </div>
                  <FieldDescription>
                    留空时优先使用应用私有 runtime；未安装时检测
                    PATH。指定外部路径必须为受支持的官方 Serena 1.7.0 或以上。
                  </FieldDescription>
                </Field>
                <div className="detected-path">
                  <span>当前检测</span>
                  <code>{state.installation?.path ?? "未发现"}</code>
                </div>
              </FieldGroup>
            </div>

            <div className="settings-section">
              <header>
                <span>03</span>
                <div>
                  <h2>Serena 内部服务</h2>
                  <p>提供代码分析能力，由 MCP 连接入口调用</p>
                </div>
              </header>
              <FieldGroup className="settings-body field-stack">
                <Field className="max-w-md">
                  <FieldLabel htmlFor="serena-port">内部服务端口</FieldLabel>
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
                        "端口已保存；重新启动 Serena 后生效。",
                      )
                    }
                    onKeyDown={(event) => {
                      if (event.key === "Enter") event.currentTarget.blur();
                    }}
                  />
                  <FieldDescription>
                    仅供本机内部通信，无需填入 Cloudflare。允许范围
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
                  hint="默认关闭；也可从 Serena 页面或托盘手工打开"
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
                <Field className="max-w-md">
                  <FieldLabel htmlFor="broker-port">连接入口端口</FieldLabel>
                  <Input
                    type="number"
                    min={1024}
                    max={65535}
                    id="broker-port"
                    value={brokerPort}
                    disabled={
                      !!brokerController.busy ||
                      brokerController.broker?.running
                    }
                    onChange={(e) => setBrokerPort(Number(e.target.value))}
                  />
                  <FieldDescription>
                    Cloudflare MCP upstream
                    使用此入口，本机地址可在首页复制。停止入口后可修改端口和访问范围，重新启用时生效。
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
                        brokerController.broker?.running
                          ? "MCP 连接入口已停止"
                          : "MCP 连接入口已启用",
                      )
                    }
                    aria-busy={updatingBroker}
                  >
                    {updatingBroker ? (
                      <>
                        <Spinner data-icon="inline-start" aria-hidden="true" />
                        处理中…
                      </>
                    ) : brokerController.broker?.running ? (
                      "停止连接入口"
                    ) : (
                      "启用连接入口"
                    )}
                  </Button>
                </div>
              </FieldGroup>
            </div>
          </section>
  );
}

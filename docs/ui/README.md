# Serena Desktop UI Reference

`docs/ui` 保存 Serena Desktop 的设计参考资产；它不定义产品行为或后端能力。

## Source of Truth

发生冲突时，按以下优先级处理：

1. 现有 Serena Desktop 实现与后端 Contract（行为）
2. [DESIGN.md](DESIGN.md)（视觉系统）
3. 对应页面目录中的 `screen.png`（页面视觉）
4. 对应页面目录中的 `code.html`（布局、间距、尺寸参考）

`code.html` 是 Stitch 导出参考，不是生产代码。不得据此虚构后端能力、业务字段或状态。

## 页面与实际 React 组件

| 设计资产目录 | 实际 React 组件 |
|---|---|
| `home` | `src/ProjectPanel.tsx` |
| `status` | `src/features/status/StatusPage.tsx` |
| `settings` | `src/features/settings/SettingsPage.tsx` |
| `logs/*` | `src/McpLogs.tsx` |
| `agent/list` | `src/AgentPanel.tsx` |
| `agent/detail` | `src/ExecutionDetails.tsx` |
| `remote-access/*` | `src/RemoteAccessPage.tsx` |

## 资产目录

```text
components/gallery/                 Component gallery
home/                               首页
status/                             服务状态
settings/                           设置
logs/default/                       日志默认态
logs/multi-select/                  日志多选态
agent/list/                         Agent 列表
agent/detail/                       Agent 详情
remote-access/quick-tunnel/         快速隧道
remote-access/ngrok-ready/          ngrok 隧道已就绪的配置/结果视图
remote-access/ngrok-connected/      ngrok 公网 MCP 已连接的运行/诊断视图
remote-access/self-hosted-https/    自建 HTTPS
remote-access/mcp-only/             仅 MCP
stitch/design-tokens.md             Stitch 导出的参考 token
archive/agent-legacy/               旧版 Agent 参考；不作为当前实现依据
```

除 `stitch/design-tokens.md` 外，每个设计页面目录均保留原始 `code.html` 与 `screen.png`。`DESIGN.md` 是唯一权威视觉规范，内容不应由 Stitch 导出覆盖。

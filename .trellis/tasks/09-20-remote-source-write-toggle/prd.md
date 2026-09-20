# Remote Source Write 开关

## Goal

为本地用户提供持久化开关，控制 Remote MCP 是否公开并授权六个既有 Source Write 工具，默认关闭且重新连接生效。

## Requirements

- 在 `ManagerConfig` 中增加持久化的 `remoteSourceWriteEnabled` 布尔字段，缺失字段和默认值均为 `false`。
- 仅本地 Tauri `save_config` 可写入该字段；Remote MCP 不增加任何配置写入工具。
- Remote MCP 的 `tools/list` 仅在字段为 `true` 时公开六个既有 Source Write 工具。
- Dispatcher 必须在工具分派前再次检查该字段；关闭后的旧 catalog 调用不可进入写 Handler，并保持现有公共错误协议。
- 配置修改后仅影响新建 Broker / MCP Tool Registry；界面提示重新连接 MCP 客户端后生效，不实现 `tools/list_changed`。
- 在本地设置页增加指定文案的 Switch，复用既有保存路径和设计系统。
- 不改变 Source Write Handler、安全校验、Workspace authority、OCC 或 Agent/Remote 生命周期。

## Acceptance Criteria

- [ ] 旧配置 JSON 缺失字段时反序列化为 `false`，save/load 后保持状态。
- [ ] OFF 状态的 `tools/list` 不含六个写工具，ON 状态含全部六个及既有 schema。
- [ ] OFF、以及 ON 后切回 OFF 时，对六个旧工具名的直接调用均 fail closed，不能进入 Handler。
- [ ] ON 状态的 create/write/replace 调用能够复用现有 Handler。
- [ ] Source Read、Git、CodeGraph、Agent catalog 不受开关影响。
- [ ] 设置页的开关仅走本地 `save_config`，并显示重新连接提示。
- [ ] 完成用户指定的 Rust、前端、构建和 diff 验证；不 commit、不 push。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.

## 2026-09-20 追加：项目选择与工作区菜单

- “选择项目”弹层仅用于选择 Desktop 工作区，不显示项目管理、重命名、移除或排序操作。
- 侧栏工作区三点菜单保持编辑和删除能力；打开时三点本身无背景色，菜单右缘对齐工作区行并扩大宽度。
- 菜单打开期间，工作区行保持既有 hover 背景色。

## 2026-09-20 追加：MCP 日志详情

- 日志列表保持紧凑；请求、工具调用与错误的扩展诊断仅在选中后的日志详情中展示。
- 工具调用详情展示工具名、安全参数、阶段、结果、耗时及可用错误信息；HTTP 请求详情展示方法、路径、状态和既有安全诊断字段。
- 日志不得记录或展示工具内容、搜索词、提示词、密码、Token 或其他凭据原文；这些参数显示为“已隐藏”。
- 保持历史纯文本日志可读、可筛选和可复制。

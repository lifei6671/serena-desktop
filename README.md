# Serena Desktop

Windows 上的 Serena 桌面管理器，按 [V0.3 技术方案](docs/technical-design-v0.3.md) 提供本地项目管理和统一 MCP 入口。保留检测、安装、启停、Dashboard、托盘、单实例及登录自启。

## 使用

1. 在“Serena 服务管理与诊断”中检测或安装官方 Serena，并启动服务。Git 需要预先安装。
2. 在已有 Git 仓库的根目录打开 PowerShell，执行 `serena project create --index`，为当前仓库创建 Serena 项目配置并建立索引，完成后点击首页“同步项目”。如果仓库已有 `.serena/project.yml`，请改用 `serena project index`。找不到 `serena` 命令时，展开页面中的对应提示，使用 Desktop 检测到的可执行文件和同步配置目录，无需把受管安装加入 PATH。
3. 启动时自动同步，也可手动刷新。读取 `SERENA_HOME/serena_config.yml`（未设置时为 `~/.serena/serena_config.yml`）及 Desktop 独立配置目录中的登记表，按完整路径合并，只纳入已有 `.serena/project.yml` 的项目。无效或失效路径显示跳过原因。同步不激活项目、不启动语言服务、不扫描仓库，也不修改 Serena 配置。选中项目可手动激活、切换或取消激活；初始化与索引在终端完成。
4. 启用 MCP Broker。默认本机地址为 `http://127.0.0.1:9120/mcp`；现有 `port` 字段仍代表内部 Serena 端口（默认 9121）。

所有 MCP 客户端共享一个活动项目。无活动项目时仅管理工具可用；准备阶段失败保留旧绑定；Serena 激活开始后的失败会清空绑定，需要重新激活。Desktop 不再提供手动添加、初始化、索引或移除登记的入口；远程只能通过同步列表中的 ID 激活，不能传任意 root。

Cloudflare Tunnel 的 upstream 应指向 Broker 的本机地址，并将 HTTP Host Header 设置为 `127.0.0.1`（SDK 默认检查 loopback Host）。先完成本机验证，再切换外部入口；本次开发不自动修改现有 Tunnel。Serena 内部端口不作为客户端入口。

## 官方 Serena 运行环境

受管安装固定为官方 `serena-agent==1.7.0`，Python 3.13。2026-09-07 已完成此版本的真实 CLI/MCP 集成验证；不使用原先计划但未发布的 Enhanced fork，不在运行时静默升级。

安装仅给 uv 子进程设置 `UV_TOOL_DIR=<app-data>/runtime/uv-tools` 和 `UV_TOOL_BIN_DIR=<app-data>/runtime/bin`，不覆盖用户全局安装或修改 PATH。缺少 uv 时沿用 winget 安装入口。

`serenaPath` 留空依次发现 Managed 与 PATH；显式外部路径不兼容时不会自动回退。旧版或版本不明的 Serena 不能启动，需官方 1.7.0 或更新正式版本；外部更新版本仍需通过工具契约检查。

Desktop 的 MCP 服务使用独立的 `runtime/serena-home` 配置目录，启动时保留其中的项目登记表。启动设置 `trusted_project_path_patterns: []`，不执行项目 activation_command；不修改用户其他 Serena 实例的全局配置。语言服务仍可能需要对应语言运行环境。

首页提供项目操作；“Serena 面板”保留官方 Dashboard；设置保存后如果影响 Serena 启动参数，会停止当前服务，需要重新启动和激活。手动启动显示窗口，登录自启隐藏窗口，退出回收受管进程。

## MCP 工具契约

固定公开 18 个工具，通过 `tools/list` 获取参数和返回 Schema。新增工具在 `src-tauri/src/mcp/registry.rs` 静态注册；内嵌工具增加 Rust 处理函数，第三方 MCP 增加具体 Adapter。CodeGraph 使用专用 stdio Adapter，不引入运行时插件配置或通用 MCP 聚合框架。

7 个 `source_*` 工具的描述从当前 Serena 实例的 `tools/list` 读取，逐字传递，不缩写、翻译或追加文字；公开工具名称、参数与返回 Schema 仍遵循 Broker 契约。4 个 `workspace_*` 和 6 个 `git_*` 工具由本地实现，描述分别包含“【做什么】”“【什么时候使用】”“【关键约束】”。获取工具列表不激活或切换项目。Serena 未运行、读取失败或工具不兼容时，`tools/list` 明确报错，不返回占位描述或不完整列表；连接客户端前须先启动 Serena。

| 工具 | 参数（未注明的均可选） |
|---|---|
| `workspace_list` / `workspace_current` / `workspace_deactivate` | 无 |
| `workspace_activate` | `id` 必填，来自项目列表 |
| `source_read_file` | `relative_path` 必填，`start_line` / `end_line`（从 0 开始） |
| `source_list_dir` | `relative_path` 必填，`recursive` 默认 false |
| `source_find_file` | `file_mask` 必填，`relative_path` 默认根目录 |
| `source_search_pattern` | `substring_pattern` 必填，`relative_path` |
| `source_symbols_overview` | `relative_path` 必填，`depth` |
| `source_find_symbol` | `name_path_pattern` 必填，`relative_path` / `depth` / `include_body` |
| `source_find_references` | `relative_path` / `name_path` 必填 |
| `git_status` / `git_branch` / `git_worktree_list` | 无业务参数 |
| `git_diff` | `scope`: unstaged（默认）/ staged / all，`path` |
| `git_log` | `reference` 默认 HEAD，`count` 默认 20、最大 100，`path` |
| `git_show` | `reference` 默认 HEAD，`path` |
| `codegraph_explore` | `query` 必填，`maxFiles` 可选（默认 12），不接受 `projectPath` |

Source 和 Git 均接受 `max_bytes`：读取文件默认 32 KiB、最大 128 KiB，其余默认 64 KiB、最大 256 KiB。Git 在读取输出时限额，截断返回提示；Source 优先使用下游预算，超过可接受大小明确报错。相对路径不得越出仓库，Git 历史路径不要求当前存在。

成功返回 JSON 文本及同内容 `structuredContent`：查询结果为 `{workspace, text, truncated, hint?}`；管理结果为 `{workspaces, truncated}` 或 `{activeWorkspace, status?, truncated}`。`text` 保留下游文本/JSON；调用失败使用 MCP `isError`，参数/未知工具使用 SDK 协议错误。

调用默认最多 60 秒，取消后有界等待清理。Source 超时或取消后的会话失效，需重新激活；Git 进程超时 30 秒；本地创建/索引最长 10 分钟且可取消。停止 Broker 清除活动项目。活动项目、PID、会话和索引显示状态不跨应用启动持久化。

## CodeGraph 接入

当前 Adapter 已核对本机 CodeGraph 1.6.0 的 CLI/MCP 契约。预先安装 CodeGraph，并确保启动 Desktop 的环境能在 PATH 找到 `codegraph`。对外只增加固定的 `codegraph_explore(query, maxFiles?)`，`maxFiles` 由 Adapter 显式传入，默认固定为 12，不接受 `projectPath`。结构探索、跨模块调用链与影响分析使用 CodeGraph；已知文件或 Symbol、精确读取与直接 references 优先使用 `source_*`。

激活仍只接受登记项目 ID。Serena、Git、CodeGraph 使用同一规范化 root；每次 CodeGraph 绑定保存 Workspace ID、root 和单调递增 generation，状态查询及调用必须同时匹配。Workspace 激活依旧串行；图启动在该 generation 自己的异步任务中执行，不等待图就绪，也不消耗 Serena 的激活时间预算。激活成功可返回 `codegraph.status = starting`，Source/Git 已可使用；通过 `workspace_current` 观察后续状态。新绑定提交后释放旧客户端；旧启动结果只持有旧 binding，不能写入新的 ActiveWorkspace。取消激活或停止 Broker 会取消自己的启动任务并释放 client/proxy，不强杀共享 daemon。

Desktop 不安装或自动初始化索引。当前活动 Workspace 根目录必须已经存在对应 CodeGraph index（`.codegraph/codegraph.db`），不从父级 Workspace 自动发现子项目索引。Adapter 检查 SQLite 文件头；子进程启动、MCP initialize、tools/list 成功且工具契约兼容即为 Ready，不执行任何语义查询探针。上游 properties 必须包含 query/maxFiles，required 只能包含 query；索引实际可用性由第一次真实查询确认。缺少数据库为 `not_initialized`；损坏文件头、启动/握手/契约失败为 `start_failed`；找不到 CLI 为 `unavailable`；连接丢失为 `runtime_lost`。初始化最多 25 秒。CodeGraph 自己维护 shared daemon、watcher 和 idle cleanup，Desktop 不建进程池或自动索引任务。

CodeGraph 在独立子进程运行，崩溃不会退出 Desktop、停止 Broker 或清除活动项目。查询发现启动失败或运行连接丢失时，最多恢复一次，恢复后最多重试当前查询一次；普通上游工具错误、取消、超时不在当前调用中触发重启。恢复尝试之间至少间隔 30 秒，并发查询不会形成重启风暴。没有后台恢复循环；修复安装或索引后也可显式重新激活。每次图查询含排队/恢复的总预算为 50 秒，单次上游查询最多 20 秒；超时或取消不复用未完成请求的会话。查询期间持有 Workspace 读锁，激活/停用会等待该查询释放锁，单次查询的等待受上述 50 秒总预算约束。发现 Serena Binding 失效时，获取写锁后重新核对 Workspace identity、generation、PID 与失效状态，再清除整个活动 Workspace。

`workspace_activate` / `workspace_current` 的 `codegraph` 字段包含 `status`、`workspaceId`、`root`、`generation`、`lastError`、`lastFailureAt`。图查询成功保持 `{workspace, text, truncated}`，保留上游文本及索引陈旧提示，超过 256 KiB 明确报错。图查询业务错误使用 MCP `isError: true`，文本和 `structuredContent` 都返回 `{error: {code, message, workspace: {id, name} | null, recoverable}}`，不返回原始异常或堆栈。已有工具的错误与返回语义不变；无效参数仍是 MCP 协议错误。

| CodeGraph 错误码 | 含义 |
|---|---|
| `WORKSPACE_NOT_ACTIVE` | 未激活或绑定已失效 |
| `CODEGRAPH_NOT_INITIALIZED` | 当前项目没有可加载的索引 |
| `CODEGRAPH_STARTING` | 正在启动或恢复 |
| `CODEGRAPH_START_FAILED` | 索引、启动、握手或探针失败 |
| `CODEGRAPH_RUNTIME_LOST` | 连接丢失，恢复失败或处于冷却期 |
| `CODEGRAPH_UNAVAILABLE` | CLI 不可用或 ID/root/generation 不匹配 |
| `CODEGRAPH_CANCELLED` / `CODEGRAPH_TIMEOUT` | 取消或达到查询预算 |
| `CODEGRAPH_UPSTREAM_ERROR` | 上游工具报错或返回不支持的内容 |
| `CODEGRAPH_OUTPUT_LIMIT` | 返回文本超过 256 KiB |

生命周期诊断写入现有本地 MCP 日志，包括 Workspace ID、generation、启动、就绪、绑定切换/释放、观察到的退出、初始化/transport 失败、恢复及过期任务丢弃。stderr 持续排空，每个 runtime 最多记录 8 KiB，日志沿用最近 500 条上限；不记录查询参数、工具结果或完整环境。能力状态不跨应用重启持久化。固定工具目录不受 CodeGraph 状态影响；现有 `tools/list` 仍需要 Serena 运行以提供原始 Source 描述。

## 开发

需要 Node.js `^20.19.0` 或 `>=22.12.0`、npm、Rust 和 Tauri 2 的 Windows 构建依赖。

```powershell
npm install
npm run tauri dev
```

验证与构建：

```powershell
npm run lint
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
npm run tauri build
```

当前 `tauri.conf.json` 关闭安装包 bundling，`npm run tauri build` 会生成单个 Windows 可执行文件：

```text
src-tauri/target/release/serena-desktop.exe
```

## 自动发布

`.github/workflows/release.yml` 监听所有 tag 的 push 事件，在 Windows x64 上安装依赖、运行前端 lint 和 Rust 测试、构建 Release 二进制，然后创建对应 tag 的 GitHub Release 并上传 `serena-desktop.exe`，自动生成发布说明。任何检查或构建失败都会阻止发布。

发布前先更新并提交项目版本号（`package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`），再对包含工作流和待发布代码的提交打 tag 并推送，例如：

```powershell
git tag v0.1.0
git push origin v0.1.0
```

工作流使用自动提供的 `GITHUB_TOKEN` 和 `contents: write` 权限，不需要配置个人 Token。所有 tag 默认发布为正式 Release；tag 不会自动修改程序内版本号。重跑时会更新已有 Release 的同名附件（仓库启用不可变 Release 时，已发布附件不能覆盖，需使用新 tag）。

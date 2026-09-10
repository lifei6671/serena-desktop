# Serena Desktop

## 让 AI 连接你的代码，让你专注真正的开发。

**面向 Windows 开发者的本地代码工作台。** 将 Serena 代码检索、Git 查询、CodeGraph 结构探索和 Codex Agent 任务管理汇集到一个桌面应用中，为支持 MCP 的 AI 客户端提供统一入口。

少一点服务启停、端口查找和窗口切换，多一点对项目本身的关注。

[下载 Windows 版本](https://github.com/lifei6671/serena-desktop/releases) · [快速开始](#快速开始) · [反馈与建议](https://github.com/lifei6671/serena-desktop/issues)

![Serena Desktop 首页：当前工作区、服务状态与 MCP 连接地址](docs/static/home-1.png)

## 从连接项目，到理解代码，一个入口就够了

当 AI 需要了解你的项目，真正有用的是准确的代码、符号关系和 Git 变更。Serena Desktop 把这些能力集中起来，让你在桌面选择工作区，再通过统一 MCP 地址交给客户端使用。

| 你想做的事 | Serena Desktop 带来的便利 |
|---|---|
| 让 AI 读取和理解项目 | 通过 Serena 读取文件、检索代码、定位符号与引用 |
| 了解最近改了什么 | 查询 Git 状态、差异、提交历史、分支与 worktree |
| 看清跨模块关系 | 接入 CodeGraph，探索代码结构、调用链与变更影响 |
| 在多个项目间切换 | 同步已初始化的项目，在首页查看和切换当前工作区 |
| 管理本地 Agent 任务 | 在 Agent 页面创建 Codex 任务，查看状态与结果 |
| 减少日常维护操作 | 集中管理服务启停、登录自启、托盘与连接配置 |

CodeGraph 需单独安装并初始化项目索引；Agent 需要本机可用的 Codex 环境。所有 MCP 客户端共享当前活动项目，切换工作区会影响它们后续的工具调用。

## 看得见的状态，找得到的问题

服务有没有启动、运行的是哪个版本、管理面板在哪里——打开状态页即可集中查看。需要排查时，可以重新检测环境、重启服务或打开日志目录，减少来回寻找信息的时间。

![服务状态页：运行状态、依赖版本、管理面板与诊断入口](docs/static/home-2.png)

## 按你的习惯，融入日常开发

登录 Windows 后启动、自动启动 Serena、关闭窗口进入托盘，都可以按需设置。MCP 地址在首页直接复制，也可以按需开启局域网连接，让同一可信网络中的另一台电脑访问这台机器的工作区。

![设置页：启动选项、Serena 运行环境与 MCP 连接配置](docs/static/home-3.png)

## 把 Agent 任务放在项目身边

在当前工作区描述任务，启动本地 Codex Agent，并在同一页面查看最近任务、执行状态和结果详情。任务与项目放在一起，回看时更容易找到上下文。

![Agent 工作台：当前工作区、新建任务与最近任务结果](docs/static/home-4.png)

## 快速开始

### 1. 准备桌面应用

前往 [Releases](https://github.com/lifei6671/serena-desktop/releases) 获取 Windows 可执行文件。预先安装 Git，然后在应用中检测或安装官方 Serena，并启动服务。

### 2. 连接你的第一个项目

在 Git 仓库根目录打开 PowerShell，初始化 Serena 项目并建立索引：

```powershell
serena project create --index
```

如果已有 `.serena/project.yml`，则执行：

```powershell
serena project index
```

返回首页，点击“同步项目”，选择并激活项目。若终端找不到 `serena` 命令，展开首页的初始化提示，使用应用检测到的可执行文件路径和同步配置目录。

### 3. 让 AI 客户端接入

在设置中启用“MCP 连接入口”，将首页显示的地址填入支持 HTTP MCP 的客户端：

```text
http://127.0.0.1:9120/mcp
```

保持 Serena 服务运行，即可通过客户端查询当前项目的代码与 Git 信息。具体配置字段以客户端要求为准。

> 默认仅允许本机连接。局域网访问需手动开启；服务没有内置认证，请仅在可信网络使用。项目初始化与索引仍在终端完成。

## 在 ChatGPT 中连接

把 Serena Desktop 添加为 ChatGPT 插件，在对话中直接查询当前项目的代码、符号与 Git 变更。以下按截图中的“服务器 URL”方式配置。

### 准备连接地址

先保持 Serena Desktop、Serena 服务和 MCP 连接入口运行，并激活需要使用的项目。准备一个 ChatGPT 可以访问的 **HTTPS MCP 地址**；表单中不能直接填写本机的 `127.0.0.1` 或局域网 IP。

如果使用 Cloudflare Tunnel，将上游指向 `http://127.0.0.1:9120/mcp`，并将 HTTP Host Header 设置为 `127.0.0.1`。在 ChatGPT 中填写最终对外的完整 MCP 地址，例如 `https://mcp.example.com/mcp`（请替换为自己的实际地址）。

> Serena Desktop 没有内置认证。对外接入时，请配合具有认证能力的 MCP 网关使用；仅建立 Cloudflare Tunnel 并不等于已经配置认证，不要将无保护的工作区入口直接暴露到公网。

### 1. 打开插件页面，创建连接

在 ChatGPT 中打开“设置 → 安全与登录”，开启“开发者模式”，保留 CSP 安全检查。然后进入“插件”页面，选择顶部的“插件”标签，点击“搜索插件”右侧的 **＋**。

### 2. 填写新插件信息

| 表单项目 | 填写说明 |
|---|---|
| 名称 | 填写 `Serena Desktop`，方便在对话中识别 |
| 描述（可选） | 可填写“连接本地项目，查询代码、符号引用和 Git 变更” |
| 连接 | 选择“服务器 URL”，粘贴完整的 HTTPS MCP 地址，保留实际路径 |
| 身份验证 | 按网关实际配置选择；网关提供 OAuth 时选择 OAuth 并完成授权。只有端点确实不要求认证时才选择“无认证”，不要照搬其他项目的设置 |
| 风险提示 | 阅读提示，确认信任自己的服务后，勾选“我了解并希望继续” |
| 创建 | 点击“创建”，等待 ChatGPT 连接服务并发现工具 |

| 第一步：打开插件，点击加号 | 第二步：填写信息，创建插件 |
|:---:|:---:|
| ![ChatGPT 插件页面：选择插件标签，点击右侧加号新增插件](docs/static/chatgpt-1.png) | ![ChatGPT 新插件表单：填写名称、完整 MCP 地址、认证方式并确认创建](docs/static/chatgpt-2.png) |

创建完成后，新建一段对话，从工具菜单添加 **Serena Desktop**，试着发送：

> 请使用 Serena Desktop 查看当前活动项目，并总结当前 Git 工作区的变更。

看到返回的项目与 Serena Desktop 首页一致，即可继续围绕这个项目提问。若连接失败，先检查服务是否运行、HTTPS 地址是否可达，以及网关认证是否匹配。

开发者模式是否可用取决于账户与工作区策略，界面入口也可能随版本调整。连接要求与操作流程可参阅 [OpenAI 官方插件连接指南](https://developers.openai.com/plugins/deploy/connect-chatgpt)。

## 技术与开发文档

需要了解接入参数、运行环境或参与开发？以下保留完整说明，按需展开即可。

- [V0.3 技术方案](docs/technical-design-v0.3.md)

<details>
<summary>使用</summary>

1. 在“Serena 服务管理与诊断”中检测或安装官方 Serena，并启动服务。Git 需要预先安装。
2. 在已有 Git 仓库的根目录打开 PowerShell，执行 `serena project create --index`，为当前仓库创建 Serena 项目配置并建立索引，完成后点击首页“同步项目”。如果仓库已有 `.serena/project.yml`，请改用 `serena project index`。找不到 `serena` 命令时，展开页面中的对应提示，使用 Desktop 检测到的可执行文件和同步配置目录，无需把受管安装加入 PATH。
3. 启动时自动同步，也可手动刷新。读取 `SERENA_HOME/serena_config.yml`（未设置时为 `~/.serena/serena_config.yml`）及 Desktop 独立配置目录中的登记表，按完整路径合并，只纳入已有 `.serena/project.yml` 的项目。无效或失效路径显示跳过原因。同步不激活项目、不启动语言服务、不扫描仓库，也不修改 Serena 配置。选中项目可手动激活、切换或取消激活；初始化与索引在终端完成。
4. 启用 MCP Broker。默认本机地址为 `http://127.0.0.1:9120/mcp`；现有 `port` 字段仍代表内部 Serena 端口（默认 9121）。

所有 MCP 客户端共享一个活动项目。无活动项目时仅管理工具可用；准备阶段失败保留旧绑定；Serena 激活开始后的失败会清空绑定，需要重新激活。Desktop 不再提供手动添加、初始化、索引或移除登记的入口；远程只能通过同步列表中的 ID 激活，不能传任意 root。

局域网连接：在设置的“MCP 连接入口”中先停止入口，开启“允许局域网连接”，再启用入口。默认仍仅监听 `127.0.0.1`；开启后监听所有 IPv4 网卡（`0.0.0.0`）。首页列出启动时发现的本机 IPv4 地址，复制与另一台电脑同网段的 `http://<IP>:<端口>/mcp` 使用；切换网络后需重新启用入口。旧配置缺少 `broker.allowLan` 时默认为 `false`。服务没有内置认证，能访问端口的设备可以读取和切换共享工作区，请仅在可信网络开启；Windows 防火墙如拦截连接，需要手动允许对应端口，应用不会自动修改规则。当前不提供 IPv6 监听或自定义域名配置，Host 校验保留且只额外允许启动时枚举的本机 IPv4 地址。

Cloudflare Tunnel 的 upstream 应指向 Broker 的本机地址，并将 HTTP Host Header 设置为 `127.0.0.1`（SDK 默认检查 loopback Host）。先完成本机验证，再切换外部入口；本次开发不自动修改现有 Tunnel。Serena 内部端口不作为客户端入口。

</details>

<details>
<summary>官方 Serena 运行环境</summary>

受管安装固定为官方 `serena-agent==1.7.0`，Python 3.13。2026-09-07 已完成此版本的真实 CLI/MCP 集成验证；不使用原先计划但未发布的 Enhanced fork，不在运行时静默升级。

安装仅给 uv 子进程设置 `UV_TOOL_DIR=<app-data>/runtime/uv-tools` 和 `UV_TOOL_BIN_DIR=<app-data>/runtime/bin`，不覆盖用户全局安装或修改 PATH。缺少 uv 时沿用 winget 安装入口。

`serenaPath` 留空依次发现 Managed 与 PATH；显式外部路径不兼容时不会自动回退。旧版或版本不明的 Serena 不能启动，需官方 1.7.0 或更新正式版本；外部更新版本仍需通过工具契约检查。

Desktop 的 MCP 服务使用独立的 `runtime/serena-home` 配置目录，启动时保留其中的项目登记表。启动设置 `trusted_project_path_patterns: []`，不执行项目 activation_command；不修改用户其他 Serena 实例的全局配置。语言服务仍可能需要对应语言运行环境。

首页提供项目操作；“Serena 面板”保留官方 Dashboard；设置保存后如果影响 Serena 启动参数，会停止当前服务，需要重新启动和激活。手动启动显示窗口，登录自启隐藏窗口，退出回收受管进程。

</details>

<details>
<summary>MCP 工具契约</summary>

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

</details>

<details>
<summary>CodeGraph 接入</summary>

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

</details>

<details>
<summary>开发</summary>

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

</details>

<details>
<summary>自动发布</summary>

`.github/workflows/release.yml` 监听所有 tag 的 push 事件，在 Windows x64 上安装依赖、运行前端 lint 和 Rust 测试、构建 Release 二进制，然后创建对应 tag 的 GitHub Release 并上传 `serena-desktop.exe`，自动生成发布说明。任何检查或构建失败都会阻止发布。

发布前先更新并提交项目版本号（`package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`），再对包含工作流和待发布代码的提交打 tag 并推送，例如：

```powershell
git tag v0.1.0
git push origin v0.1.0
```

工作流使用自动提供的 `GITHUB_TOKEN` 和 `contents: write` 权限，不需要配置个人 Token。所有 tag 默认发布为正式 Release；tag 不会自动修改程序内版本号。重跑时会更新已有 Release 的同名附件（仓库启用不可变 Release 时，已发布附件不能覆盖，需使用新 tag）。

</details>

## 一起让本地 AI 开发更顺手

[下载体验](https://github.com/lifei6671/serena-desktop/releases)，从连接你的第一个项目开始。欢迎通过 [Issues](https://github.com/lifei6671/serena-desktop/issues) 分享使用反馈；如果这个项目对你有帮助，也欢迎点亮 Star。

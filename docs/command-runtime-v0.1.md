# SerenaDesktop Command Runtime V0.1

## 1. 定位

Command Runtime 为已登记 Workspace 提供受管命令执行与可持久化执行凭据。它与 Agent Execution、Workspace Capability Runtime 分离：

```text
Work
├── Agent Execution -> AgentProvider / Codex Runtime
└── CommandRun      -> Command Runtime / Managed Process
```

CommandRun 不复用 `executions` 状态机，不取得 Agent Workspace Claim，也不实现新的 Provider。

## 2. 公共能力

Remote MCP 使用两个入口：

- `command_execute`：`start`、`cancel`；
- `command_query`：`get`、`list`、`observe`、`output`。

Remote 默认关闭，只有本机配置 `remoteCommandExecutionEnabled=true` 时才 advertise/dispatch。Local Desktop 可以通过同一 Product Service 调用，不依赖 Remote 开关。

## 3. Start Contract

`start` 必须提供：

- `workspaceId`：只允许 Local Human 已登记 Workspace；
- `requestKey`：Workspace 内幂等；
- `spec`：
  - `mode=process`：`executable + args[]`；
  - `mode=shell`：`command`；
- 可选 `relativeCwd`，默认 Workspace Root；
- 可选 `env`，只影响本次子进程；
- `timeoutMs` 默认 30s、最大 24h；
- `executionMode=auto|sync|async`；
- `yieldTimeMs` 默认 5s、最大 30s。

公共请求永远不接受 absolute root/cwd。服务端通过：

```text
workspaceId
  -> WorkspaceResolver
  -> WorkspaceLease
  -> WorkspacePathResolver(relativeCwd)
  -> Managed Process
```

## 4. Shell 与 Process

Process 模式用于参数边界清晰的命令；Shell 模式用于 Agent 开发工作流中的管道、重定向和组合命令。Shell 内容不做启发式危险命令分类，权限由本机显式 Remote 开关、已登记 Workspace、操作系统用户权限及部署环境共同限定。

Windows Shell 顺序：PowerShell 7 -> Windows PowerShell -> cmd。macOS Shell 从当前系统用户账户的 passwd 记录读取登录 Shell，要求其为绝对路径且是可执行文件；不可用时回退 `/bin/sh`。以所选 Shell 的 `-c` 执行，不显式加载 profile/dotfiles，也不以 `$SHELL` 为唯一来源。Shell 和 Process 使用同一个平台 Managed Process ownership 层。

macOS Process 保持 `executable + args[]` 的 argv 边界，不经过 Shell。启动后首次需要 PATH 时，从当前账户 passwd 登录 Shell 以有界 `-ilc` 探测交互式登录 PATH；只提取 PATH，不导入 Shell 的完整环境。超时或失败时回退到 Host PATH、系统目录、Homebrew 常见目录和用户常见开发工具目录；结果在 Host 进程内缓存，用户 PATH 目录优先。调用方显式 `env.PATH` 对单个 CommandRun 完整覆盖默认值，同时控制可执行文件发现和子进程 PATH。

## 5. 生命周期

短命令在 `auto` 模式下等待最多 `yieldTimeMs`，完成则直接返回；超过阈值返回 running CommandRun，后续通过 bounded `command_query.observe` 查询。MCP 请求取消不等于 CommandRun 取消。

CommandRun 状态：

```text
starting -> running -> completed | failed
                    -> cancelling -> cancelled | unknown
                    -> interrupted | unknown
```

Host restart：Windows Job-at-Creation + KILL_ON_JOB_CLOSE 允许把旧未决 CommandRun 收敛为 interrupted；macOS 目前没有跨 Host 重启的可验证 Process Group 终止事实，旧未决 CommandRun 保持 unknown。

## 6. 平台进程所有权

Windows 必须保持 SerenaDesktop 已验证的强约束：

```text
Create Job
  -> JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
  -> PROC_THREAD_ATTRIBUTE_JOB_LIST
  -> CreateProcessW
```

禁止使用两阶段 `CreateProcess -> AssignProcessToJobObject`。进程从第一个可运行时刻起必须属于 Command Job，Job handle 不继承给 Child。

macOS 使用 exec 前 `setsid` 建立私有 Session/Process Group，父进程冻结 Darwin leader 身份。取消、超时和 Host 关闭先向匹配身份的组发 `SIGTERM`，有限等待后必要时发 `SIGKILL`；terminal 前要求直接 child 已回收且 Process Group 已空。若 leader 已退出且残留组无法安全归属，CommandRun 保持未决并继续持有 Workspace guard，直到组空后才记录 unknown，不伪造 Windows Job 证据。持久化 `runtimePlatform=macos`、`containmentType=process_group`。

## 7. 输出

stdout/stderr 实时读取但有界保存：

- live tail：每流最大 1 MiB，完成态最多保留 32 个会话；
- 单次 MCP 输出默认 64 KiB；
- 使用客户端 cursor 读取增量输出，多客户端互不消费；
- 记录 total bytes、dropped bytes、truncated；
- 对完整原始 stdout/stderr 增量计算 SHA-256。

stdout/stderr 正文不写入 SQLite；terminal 后持久化 exit code、timeout、termination reason、总字节数和 SHA-256，形成 Command Receipt。这样 Work Evidence 可持久化，同时避免把命令输出中的 credential 永久写库。

## 8. Environment

默认只继承运行开发工具所需的最小宿主环境；不会把 Host 的完整环境复制给子进程。调用方显式 `env` 可以覆盖非保留键。SerenaDesktop OAuth、Remote Access Token 等内部凭据不会通过默认继承进入命令环境。

## 9. Work Evidence

`work_command_links` 把 CommandRun 关联到 Work。Work finish 时所有关联 Agent Execution 与 CommandRun 都必须 terminal。Acceptance 可同时记录：

```json
{
  "summary": "...",
  "executionIds": ["execution-..."],
  "commandRunIds": ["command-..."]
}
```

Command Receipt 是 Host 真实执行事实；它不等同于测试语义判定。后续 TestEvidence 可以在 Receipt 之上识别具体测试框架。

## 10. V0.1 非目标

- PTY/完整终端模拟；
- 任意 absolute cwd；
- Docker Sandbox；
- 自动危险命令分类；
- Workflow DAG；
- stdout/stderr 全量永久日志；
- CommandRun 复用 Agent Runtime/Claim/Evidence 状态机。

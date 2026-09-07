# Serena Desktop Manager × Serena Enhanced 集成技术方案 v0.3

## 1. 目标

Serena Desktop Manager 继续作为 Windows 上的轻量级 Serena Supervisor，负责：

* 检测 Serena Enhanced；
* 安装和维护 Manager 自己管理的 Serena Enhanced；
* 检测 Git 运行环境；
* 启动、停止、重启 Serena MCP Server；
* 固定使用 `chatgpt-review` Context；
* 管理端口、Dashboard、日志、托盘和 Windows 登录自启。

Desktop Manager 不实现 Git Tool，不解析 Git repository，不代理 MCP。

Git 能力全部由 Serena Enhanced 提供。

整体关系：

```text
ChatGPT
   │
   │ MCP
   ▼
Serena Enhanced
   │
   ├── Serena semantic tools
   ├── git_status
   ├── git_diff
   ├── git_log
   ├── git_show
   ├── git_branch
   └── git_worktree_list
   │
   ▼
Local Project


Serena Desktop Manager
   │
   ├── install
   ├── detect
   ├── start
   ├── stop
   ├── config
   ├── logs
   └── tray
```

---

# 2. 核心设计决策

## 2.1 Manager 使用独立 Serena 安装目录

默认不覆盖用户机器上已有的官方 Serena。

Manager 管理自己的：

```text
SerenaDesktop/
└── runtime/
    ├── uv-tools/
    └── bin/
        ├── serena.exe
        ├── serena-agent.exe
        └── serena-hooks.exe
```

实际目录使用 Tauri App Data Directory，例如：

```text
%APPDATA%/.../SerenaDesktop/runtime/
```

具体目录继续通过 Tauri path API 获取，不硬编码 `%APPDATA%`。

安装时设置：

```text
UV_TOOL_DIR=<app-data>/runtime/uv-tools
UV_TOOL_BIN_DIR=<app-data>/runtime/bin
```

因此：

```text
用户全局 Serena
        │
        ├── 可继续存在
        │
        └── 不受 Manager 修改

Manager Serena Enhanced
        │
        └── 独立运行
```

这样无需修改 Serena Fork 的：

* Python package name；
* `serena` CLI 名称；
* upstream version；
* console entry point。

---

# 3. Serena Enhanced 发布方式

## 3.1 不直接安装 main

Desktop Manager 不执行：

```text
git+https://github.com/lifei6671/serena.git@main
```

原因是 main 不具有可重复发布语义。

每个可被 Desktop Manager 使用的 Serena Enhanced 版本必须对应固定：

```text
Git Tag
```

建议版本：

```text
v1.7.1-enhanced.1
v1.7.1-enhanced.2
...
```

其中：

```text
1.7.1
```

表示基于 Serena upstream 的版本代际；

```text
enhanced.1
```

表示本 Fork 的扩展版本。

v0.3 不要求修改 Python package 内部版本号。

---

# 4. Desktop 内部发行契约

Rust 代码中定义固定发行信息，例如概念上：

```text
SERENA_ENHANCED_SOURCE =
    git+https://github.com/lifei6671/serena.git

SERENA_ENHANCED_REF =
    v1.7.1-enhanced.1

SERENA_CONTEXT =
    chatgpt-review

SERENA_PYTHON =
    3.13
```

这些值属于应用内部常量。

UI 不允许用户修改：

```text
Git repository
Git branch
Git tag
Python version
Context
```

避免把 Manager 变成通用 Python/Git 安装器。

---

# 5. 安装流程

完整流程：

```text
点击“安装 Serena Enhanced”
        │
        ▼
检测 Git
        │
        ├── 不存在
        │      └── 返回明确错误和 Git 官方入口
        │
        ▼
检测 uv
        │
        ├── 已存在
        │
        └── 不存在
               │
               ▼
         检测 winget
               │
               ├── 有
               │    └── 安装 astral-sh.uv
               │
               └── 无
                    └── 返回失败和 uv 官方入口
        │
        ▼
设置 Manager 独立 uv 环境
        │
        ▼
uv tool install
        │
        ▼
验证 serena.exe
        │
        ▼
验证 --version
        │
        ▼
验证 chatgpt-review
        │
        ▼
安装成功
```

---

# 6. Git 检测

新增：

```text
detect_git
```

内部执行：

```text
git --version
```

结果：

```text
available
missing
error
```

例如：

```json
{
  "available": true,
  "path": "C:\\Program Files\\Git\\cmd\\git.exe",
  "version": "2.51.0"
}
```

v0.3 不自动安装 Git。

原因：

* Git 本身属于开发环境基础设施；
* Git Installer 涉及 PATH、Shell Integration 等额外选项；
* 自动管理 Git 超出 Serena Supervisor 的职责。

如果 Git 缺失：

```text
Git is required by Serena Enhanced Git tools.
```

UI 提供：

```text
打开 Git 官方下载页面
```

---

# 7. uv 检测和安装

沿用 V0.1：

```text
where.exe uv
```

或实际 Rust executable discovery。

uv 不存在时：

```text
winget install
  --id astral-sh.uv
  -e
  --accept-package-agreements
  --accept-source-agreements
```

成功后重新发现 `uv.exe`。

不使用：

```text
PowerShell install script
pip
pipx
scoop
choco
```

保持安装流程唯一。

---

# 8. Serena Enhanced 安装命令

安装进程必须直接传 argv，不经过 shell。

逻辑等价于：

```text
uv tool install
  -p
  3.13
  --force
  git+https://github.com/lifei6671/serena.git@v1.7.1-enhanced.1
```

同时仅对该子进程设置：

```text
UV_TOOL_DIR=<runtime>/uv-tools
UV_TOOL_BIN_DIR=<runtime>/bin
```

不要修改用户的全局环境变量。

不要执行：

```text
uv tool update-shell
```

因为 Manager 始终使用：

```text
<runtime>/bin/serena.exe
```

绝对路径启动 Serena，不依赖系统 PATH。

---

# 9. Serena 发现模型

v0.3 定义三类 Serena。

## 9.1 Managed Enhanced

Manager 自己安装：

```text
<app-data>/runtime/bin/serena.exe
```

这是默认首选。

---

## 9.2 External Enhanced

用户通过：

```json
{
  "serenaPath": "..."
}
```

显式指定外部 Serena。

必须同时满足：

```text
文件存在
+
serena --version 成功
+
存在 chatgpt-review Context
```

才视为兼容。

---

## 9.3 Standard Serena

PATH 或其他位置发现：

```text
serena.exe
```

但不存在：

```text
chatgpt-review
```

则识别为：

```text
standard
```

而不是：

```text
error
```

Standard Serena 可以展示给用户，但 Desktop Manager 不使用它启动 ChatGPT Review MCP。

---

# 10. Serena 发现顺序

调整为：

```text
1. 用户显式 serenaPath
       │
       └── 必须通过 Enhanced capability probe

2. Manager managed runtime/bin/serena.exe
       │
       └── 必须通过 Enhanced capability probe

3. PATH / where.exe serena
       │
       ├── Enhanced → 可使用
       └── Standard → 仅报告

4. missing
```

正常情况下：

```text
serenaPath = null
```

就使用 Manager Managed Enhanced。

---

# 11. Enhanced Capability Probe

不要单纯根据：

```text
serena --version
```

判断。

因为 Fork 和 upstream 可以拥有相同版本。

检测分两步。

## Step 1

```text
serena --version
```

验证 CLI 可执行。

## Step 2

```text
serena context list
```

要求输出包含：

```text
chatgpt-review
```

并且该 Context 属于安装包内置 Context。

Manager Managed Serena 本身来自固定 Git ref，因此：

```text
managed path
+
chatgpt-review exists
```

即可认为是 Enhanced。

对于 External Serena：

必须至少存在：

```text
chatgpt-review
```

否则视为 incompatible。

---

# 12. Serena 状态模型

新增安装状态：

```text
missing
standard
enhanced
invalid
```

同时记录来源：

```text
managed
external
path
```

建议结构：

```json
{
  "installation": {
    "state": "enhanced",
    "source": "managed",
    "path": ".../runtime/bin/serena.exe",
    "version": "1.7.1.dev0",
    "context": "chatgpt-review"
  }
}
```

不需要增加复杂的 Package Metadata 模型。

---

# 13. 启动命令

V0.1：

```text
serena start-mcp-server
  --transport streamable-http
  --host 127.0.0.1
  --port <port>
  --open-web-dashboard <true|false>
```

v0.3 固定增加：

```text
--context chatgpt-review
```

完整命令：

```text
<resolved-serena-path>
  start-mcp-server
  --context chatgpt-review
  --transport streamable-http
  --host 127.0.0.1
  --port <configured-port>
  --open-web-dashboard <true|false>
```

仍然不传：

```text
--project
```

项目由 ChatGPT 后续通过：

```text
activate_project
```

动态选择。

---

# 14. 启动前检查

`start_serena` 按顺序执行：

```text
Serena Enhanced 是否有效
        │
        ▼
Git 是否可用
        │
        ▼
配置是否合法
        │
        ▼
端口是否可用
        │
        ▼
启动进程
```

如果 Git 缺失：

建议阻止启动。

原因是这个 Manager 当前管理的就是：

```text
ChatGPT Review Serena
```

Git Read Tools 属于其核心能力。

避免出现：

```text
Serena 显示 Running
但 git_status / git_diff 全部不可用
```

这种半可用状态。

---

# 15. 进程生命周期

继续沿用 V0.1：

```text
stopped
starting
running
error
```

Manager 只管理自己启动的 Serena 子进程。

Windows 使用：

```text
CREATE_NO_WINDOW
```

stdout / stderr 继续写：

```text
SerenaDesktop/logs/serena.log
```

不增加 watchdog。

不自动无限重启。

---

# 16. 配置模型

现有配置继续保持：

```json
{
  "serenaPath": null,
  "port": 9121,
  "dashboardEnabled": true,
  "openDashboardOnLaunch": false,
  "autoStartServer": true,
  "minimizeToTray": true,
  "startMinimized": false
}
```

不增加：

```text
context
gitPath
repositoryUrl
releaseTag
pythonVersion
```

这些都属于 Manager 内部实现契约。

### `serenaPath`

语义调整为：

```text
null
→ 使用 Manager Managed Enhanced

非 null
→ 使用用户指定的兼容 Enhanced Serena
```

不要再让这个字段承担普通 Serena 自动发现优先级。

---

# 17. get_app_state

继续保持一次返回系统事实。

建议增加：

```json
{
  "serena": {
    "state": "enhanced",
    "source": "managed",
    "path": "...",
    "version": "...",
    "context": "chatgpt-review"
  },
  "git": {
    "available": true,
    "path": "...",
    "version": "..."
  },
  "runtime": {
    "status": "running",
    "endpoint": "http://127.0.0.1:9121/mcp"
  }
}
```

前端不得自己推断：

```text
Enhanced
Git available
Running
```

这些事实仍由 Rust 汇总。

---

# 18. 前后端接口调整

V0.1：

```text
get_app_state
detect_serena
install_serena
start_serena
stop_serena
restart_serena
save_config
set_autostart
open_dashboard
open_log_directory
open_external_url
```

v0.3 调整为：

```text
get_app_state

detect_serena
detect_git

install_serena
repair_serena

start_serena
stop_serena
restart_serena

save_config
set_autostart

open_dashboard
open_log_directory
open_external_url
```

其中：

### `install_serena`

只安装 Manager Managed Serena Enhanced。

### `repair_serena`

重新执行固定 ref 安装。

不增加：

```text
update_serena
```

v0.3 仍然不做自动 Serena 更新。

---

# 19. Repair 语义

以下情况显示：

```text
Repair
```

而不是普通 Install：

```text
managed serena.exe 存在
但 --version 失败

managed Serena 缺少 chatgpt-review

uv environment 损坏
```

Repair：

```text
使用相同固定 Git ref
重新执行 uv tool install --force
```

不尝试局部修复 Python environment。

---

# 20. UI 调整

## 首页

Serena 区域显示：

```text
Serena Enhanced

Running
v1.7.1.dev0

Managed
C:\...\runtime\bin\serena.exe

Context
chatgpt-review
```

Git 显示为环境依赖：

```text
Git
2.51.0
Available
```

不需要独立做指标卡。

---

## Standard Serena 场景

如果检测到用户已有官方 Serena：

```text
Serena detected
Standard installation

ChatGPT Review Git tools require Serena Enhanced.
```

主操作：

```text
安装 Serena Enhanced
```

不要显示：

```text
卸载标准 Serena
替换标准 Serena
```

因为两者可以共存。

---

## Git 缺失

显示：

```text
Git is required

Serena Enhanced uses the local Git CLI for repository inspection.
```

操作：

```text
打开 Git 下载页面
重新检测
```

---

# 21. 安装日志

安装相关日志写入：

```text
app.log
```

记录：

```text
installer started
uv detected
Git detected
Serena installation started
Serena capability verified
```

允许记录：

```text
exit code
固定 command 类型
sanitized stderr
```

不记录：

```text
完整环境变量
credential helper 内容
Git credential
Token
```

---

# 22. 安全边界

继续维持 V0.1 的简单安全模型。

### 固定来源

只能安装：

```text
https://github.com/lifei6671/serena.git
```

UI 不允许输入 repository URL。

### 固定 ref

Manager release 内固定 Serena Enhanced ref。

不允许：

```text
main
用户输入 branch
用户输入 commit
```

### 固定 Context

只能使用：

```text
chatgpt-review
```

v0.3 不提供 Context 下拉框。

### 固定 Python

继续：

```text
Python 3.13
```

### 固定 Host

继续：

```text
127.0.0.1
```

---

# 23. 与官方 Serena 共存

目标状态：

```text
User Environment

PATH
└── C:\Users\...\serena.exe
    └── Official Serena


Serena Desktop Manager

AppData
└── SerenaDesktop
    └── runtime
        ├── uv-tools
        └── bin
            └── serena.exe
                └── Serena Enhanced
```

Manager 永远通过绝对路径：

```text
<AppData>/runtime/bin/serena.exe
```

启动 Managed Serena。

因此 PATH 顺序不会影响 Manager。

---

# 24. Serena Enhanced 更新策略

v0.3 不做自动更新。

每个 Desktop Manager release 固定绑定一个：

```text
SERENA_ENHANCED_REF
```

例如：

```text
Desktop Manager 0.2.0
        │
        └── Serena Enhanced v1.7.1-enhanced.1
```

未来：

```text
Desktop Manager 0.2.1
        │
        └── Serena Enhanced v1.7.1-enhanced.2
```

Manager 升级后可以检测 managed runtime 的发行标识，并提示 Repair / Upgrade。

这一机制不属于本次实现范围。

---

# 25. upstream 同步策略

Serena Fork 继续保持：

```text
oraios/serena
      │
      ▼
lifei6671/serena
      │
      ├── git_tools.py
      ├── chatgpt-review.yml
      └── targeted tests
```

同步 upstream 后：

```text
merge/rebase upstream
        │
        ▼
跑 Git Tool tests
        │
        ▼
跑 Serena quality gate
        │
        ▼
创建新的 enhanced tag
        │
        ▼
Desktop Manager 后续版本引用新 tag
```

Desktop Manager 不直接跟随 Fork main。

---

# 26. 测试

## Serena discovery

覆盖：

```text
managed enhanced exists
managed enhanced missing
managed binary invalid
external enhanced
external standard
external invalid
PATH standard Serena
```

---

## Capability probe

覆盖：

```text
--version success
--version fail

context list contains chatgpt-review
context list missing chatgpt-review
context list command fail
```

---

## Git

覆盖：

```text
git available
git missing
git --version failure
```

---

## Installer

通过 mock process runner 验证：

```text
正确 UV_TOOL_DIR
正确 UV_TOOL_BIN_DIR
正确 Git source
正确 Git ref
Python 3.13
--force
```

同时验证：

```text
无 shell
无用户可控参数进入安装命令
```

---

## Start

验证最终 argv 包含：

```text
start-mcp-server
--context
chatgpt-review
--transport
streamable-http
--host
127.0.0.1
--port
<configured>
```

---

## 共存

测试机器上存在另一个：

```text
PATH\serena.exe
```

时，Manager Managed Serena 仍然使用：

```text
runtime/bin/serena.exe
```

---

# 27. 实施顺序

建议 Codex 按以下顺序实施。

### Step 1

增加 Manager Managed Runtime：

```text
runtime/
├── uv-tools
└── bin
```

并实现路径获取。

### Step 2

调整 Serena discovery：

```text
managed
external
standard
missing
```

### Step 3

实现：

```text
detect_git
```

### Step 4

修改安装流程：

```text
uv
+
fixed Git source
+
fixed Git ref
+
isolated UV directories
```

### Step 5

实现 Enhanced capability probe：

```text
serena --version
serena context list
```

### Step 6

修改启动命令：

```text
--context chatgpt-review
```

### Step 7

更新：

```text
get_app_state
UI 状态
安装/修复提示
```

### Step 8

补测试并完成 Windows 实机验收。

---

# 28. 验收标准

v0.3 完成后必须满足：

1. 用户已有官方 Serena 时不会被覆盖；
2. Manager 可以安装自己的 Serena Enhanced；
3. Managed Serena 安装在应用私有 runtime 目录；
4. Manager 不依赖 Managed Serena 出现在 PATH；
5. 能正确检测 Git；
6. Git 缺失时不进入半可用运行状态；
7. 能区别 Enhanced 与普通 Serena；
8. 启动时固定使用：

```text
--context chatgpt-review
```

9. ChatGPT MCP 最终只能看到 `chatgpt-review` 定义的 Tool；
10. Git Tool 实现仍完全位于 Serena Fork；
11. Desktop Manager 不执行任何 Git repository 操作；
12. Serena stdout / stderr 正常进入现有日志；
13. Dashboard、端口、托盘、自启行为保持原有语义；
14. 安装、检测、启动过程均不经过 shell；
15. 前端 lint/typecheck、生产构建、Rust tests 和 `cargo check` 通过。

---

# 29. 本阶段明确不做

不增加：

* Git 自动安装；
* Serena 自动更新；
* Fork main 自动跟随；
* GitHub Release 自动检测；
* GitHub API；
* Context 编辑器；
* Context 下拉选择；
* Git Tool 配置界面；
* Serena Package 重命名；
* Serena 版本体系重构；
* MCP Tool 动态管理；
* MCP Proxy；
* Windows Service；
* 管理员权限；
* 多 Serena 实例管理。

本阶段只完成：

```text
Desktop Manager
        │
        ▼
可靠安装 Serena Enhanced
        │
        ▼
可靠识别 chatgpt-review
        │
        ▼
固定 Context 启动 MCP Server
```

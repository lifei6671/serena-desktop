# Serena Desktop Manager 技术方案 V0.1

## 1. 产品定位

Serena Desktop Manager 是 Windows 上的轻量级 Serena Supervisor，只负责检测、安装、配置和托管 Serena MCP Server。它不重新实现 Serena、不代理 MCP 协议，也不管理 Serena 项目、索引或 Memory。

V0.1 的目标是把原先依赖 PowerShell、任务计划程序、端口检查和手工查找日志的操作收拢到一个常驻托盘应用中。

## 2. 范围

V0.1 包含：

- 自动发现 Serena，显示可执行文件路径和版本；
- Serena 缺失时，经 `uv` 自动安装；缺少 `uv` 时可经 `winget` 安装 `astral-sh.uv`；
- 启动、停止、重启 Serena MCP Server；
- 使用固定监听地址 `127.0.0.1`，端口可配置且限制为 `1024..=65535`；
- 结合受管子进程和 TCP 探测显示 `stopped / starting / running / error`；
- 配置 Dashboard 是否启用、启动时是否自动打开；
- 配置 Windows 登录后启动应用、应用启动后自动启动 Serena；登录自启时隐藏窗口；
- 只允许运行一个应用实例，后续手动启动请求唤醒已有窗口；
- 关闭主窗口时隐藏到托盘，托盘退出时停止受管 Serena；
- 保存 Serena stdout/stderr 和应用日志，并打开日志目录；
- 打开 Serena Dashboard、官方文档和 GitHub。
- 在应用内嵌入当前 Serena Web Dashboard，并保留系统浏览器入口。

V0.1 不包含：

- Serena 项目、索引、语言服务或 Memory 管理；
- 任意命令输入、远程监听地址、Windows Service、管理员权限；
- 自动无限重启、更新 Serena、代理或重新实现 MCP；
- 接管由其他程序启动的 Serena 进程。

## 3. 技术栈与结构

```text
Tauri 2
├── Rust
│   ├── commands.rs    Tauri command 边界
│   ├── config.rs      Manager 配置持久化和校验
│   ├── installer.rs   uv / Serena 固定安装流程
│   ├── logs.rs        日志目录与追加写入
│   ├── serena.rs      检测、版本、进程和端口状态
│   └── tray.rs        托盘菜单与窗口生命周期
└── React + TypeScript
    ├── 首页运行台
    ├── Serena 面板
    └── 设置页
```

前端不直接创建系统进程。所有进程、文件、浏览器和 Explorer 操作均由 Rust command 完成；Windows 登录自启由官方 `tauri-plugin-autostart` 的 Rust API 管理。

## 4. 配置与数据

Manager 配置保存在 Tauri 的应用配置目录 `%APPDATA%/io.github.lifei6671.serena-desktop/config.json`，逻辑结构为：

```json
{
  "serenaPath": null,
  "port": 9121,
  "dashboardEnabled": true,
  "openDashboardOnLaunch": false,
  "autoStartServer": true,
  "minimizeToTray": true
}
```

`serenaPath: null` 表示自动发现。保存前校验端口；非空自定义路径必须指向现有文件。

`dashboardEnabled` 对应 Serena 全局配置 `%USERPROFILE%\.serena\serena_config.yml` 的顶层 `web_dashboard`。Manager 只替换或追加这一行，不重写其他 Serena 配置；变更在下次 Serena 启动时生效。`openDashboardOnLaunch` 通过 `--open-web-dashboard true|false` 传入。

日志保存在 Tauri 应用日志目录 `%LOCALAPPDATA%/io.github.lifei6671.serena-desktop/logs/`：

```text
app.log
serena.log
```

日志不记录环境变量或凭据。

## 5. Serena 发现与安装

发现顺序：

1. 用户配置路径；
2. `where.exe serena`；
3. `%USERPROFILE%\.local\bin\serena.exe`；
4. 未安装。

找到候选文件后执行 `<path> --version`；无法执行、5 秒内未返回或非零退出的候选不视为有效安装。首次启动前若 Serena 全局配置不存在，Manager 会执行官方 `serena init`，成功生成完整配置后才应用 Dashboard 设置；Manager 不自行猜造 Serena 配置结构。

安装流程只允许固定命令：

```text
检测 uv
├── 已存在：uv tool install -p 3.13 serena-agent
└── 不存在且有 winget：winget install --id astral-sh.uv -e --accept-package-agreements --accept-source-agreements
                         → 重新发现 uv
                         → uv tool install -p 3.13 serena-agent
```

任一步失败即停止，向 UI 返回可复制的错误；不尝试其他包管理器或提权。成功后重新执行 Serena 发现。

## 6. 进程与状态模型

启动命令固定为：

```text
serena start-mcp-server
  --transport streamable-http
  --host 127.0.0.1
  --port <configured port>
  --open-web-dashboard <true|false>
```

Windows 使用 `CREATE_NO_WINDOW`，stdout/stderr 均管道读取并写入 `serena.log`。启动前检查端口占用；Manager 只停止自己持有的子进程。

状态定义：

- `stopped`：没有受管进程；
- `starting`：子进程存在，端口尚未接受连接；
- `running`：子进程存在且配置端口可连接；
- `error`：启动失败、进程意外退出或进程存在但状态探测失败。

启动 command 在有限时间内轮询端口；超时或提前退出会返回明确错误，不增加 watchdog 或自动重试。运行状态由前端周期刷新，Rust 同时收割已经退出的子进程，避免把失效句柄报告为运行中。Windows 停止使用固定参数的 `taskkill.exe /PID <managed-pid> /T /F` 终止受管启动器及其 Serena Python 子进程；PID 只来自 Manager 持有的 `Child`，不按名称扫描或终止其他 Serena。

Dashboard URL 默认是 `http://127.0.0.1:24282/dashboard/index.html`。若 Serena 输出中出现实际 Dashboard URL，Manager 记录该地址并优先打开它，以覆盖默认端口被占用的情况。

## 7. 窗口与托盘生命周期

- 点击窗口关闭：当 `minimizeToTray` 为真时阻止关闭并隐藏窗口；
- 托盘“打开主界面”：显示并聚焦窗口；
- 托盘提供启动、停止、重启、打开 Dashboard、打开日志目录；
- 托盘“退出”：先停止受管 Serena，再退出应用；
- 应用只允许单实例运行；再次手动启动时显示并聚焦现有窗口，再次收到登录自启请求时保持隐藏；
- 仅 Windows 登录自启携带 `--autostart` 并隐藏主窗口，用户手动启动始终显示主窗口；
- Serena 检测和 `autoStartServer` 启动流程在后台线程执行，不阻塞主窗口创建和展示；
- 退出时立即隐藏窗口，在后台停止受管 Serena，清理完成后结束应用进程。

## 8. 前后端接口

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

`get_app_state` 一次返回配置、安装信息、运行状态、端点、Dashboard URL、日志路径和 Windows 自启状态，避免前端拼装系统事实。

## 9. UI 方向

主要使用者是刚开始或结束一段本地编码工作的开发者，最重要的动作是确认 `Desktop → Serena → MCP endpoint` 链路并立即启停。

- 首页采用单一运行台而非指标卡片网格；链路指示器是贯穿首页和托盘的识别元素；
- 设置采用窄页签和成组表单，不使用宽侧栏；
- 终端黑灰和 Windows 云母浅灰构成表面层级，绿色、琥珀色和红色只表达运行语义；
- 路径、版本、端口和 URL 使用等宽字体；
- 使用轻边框，不使用渐变、装饰阴影或无意义图表；
- WebView 使用与应用一致的窗口底色和内联启动过渡页，React 接管前不显示空白白屏；
- success/error 反馈使用固定在内容区右上角的 Toast，不挤压页面文档流，并允许手工关闭；
- 所有设置自动持久化：开关在点击后生效，路径和端口在失焦或按 Enter 后校验保存；失败时保留可编辑输入并显示 Toast；
- Serena 可执行文件既可输入，也可通过原生单文件选择对话框选择，选择后立即保存；
- 主导航在运行台后提供 Serena 面板；运行且启用 Dashboard 时嵌入实际 URL，否则显示对应引导；
- 头部导航和底部状态栏固定，中间工作区独立滚动；
- 每个异步操作提供 loading、disabled、success/error 反馈。

## 10. 安全与失败边界

- MCP 固定绑定 `127.0.0.1`，UI 不允许改为 LAN/公网地址；
- 不提供 Shell、参数或环境变量输入；
- 安装和启动均以当前普通用户身份执行；
- 自定义 Serena 路径必须是文件且能成功执行 `--version`；
- 端口范围和占用在 Rust 边界校验；
- 不记录 PATH、Token 或完整环境；
- 配置写入采用同目录临时文件后替换，避免半写入；
- 退出只终止 Manager 持有的子进程，不按名称杀进程。

## 11. 验收标准

- 已安装 Serena 可被发现并显示版本与路径；
- 缺失时可完成固定自动安装流程，失败时显示错误和官方入口；
- 启动后配置端口可连接，且没有控制台窗口；
- 停止、重启和异常退出状态正确；
- 端口修改可持久化，非法或占用端口有明确错误；
- Dashboard 默认不自动弹出，可启停并手工打开；
- 日志目录可一键打开，Serena 输出可追踪；
- 窗口关闭进入托盘，托盘能恢复窗口，退出时停止受管进程；
- Windows 登录自启和应用启动后自动启动 Serena 可分别配置；
- 登录自启时窗口保持隐藏，手动启动时窗口可见；重复启动不会产生第二个常驻进程；
- Serena 自动启动不阻塞主窗口展示，退出清理不阻塞窗口关闭反馈；
- 前端 lint/typecheck、生产构建、Rust 单元测试和 `cargo check` 通过。

## 12. 实施顺序

1. Tauri Shell、主界面、托盘和关闭隐藏；
2. Serena 检测、进程生命周期、状态和日志；
3. Manager 配置、端口和 Dashboard 设置；
4. Windows Autostart；
5. uv / Serena 自动安装和失败指引。

## 13. 当前事实依据

- Serena 安装：<https://oraios.github.io/serena/02-usage/010_installation.html>
- Serena HTTP 运行参数：<https://oraios.github.io/serena/02-usage/020_running.html>
- Serena Dashboard：<https://oraios.github.io/serena/02-usage/060_dashboard.html>
- Tauri System Tray：<https://v2.tauri.app/learn/system-tray/>
- Tauri Autostart：<https://v2.tauri.app/plugin/autostart/>

这些外部命令和插件能力以 2026-09-06 的官方文档为准。

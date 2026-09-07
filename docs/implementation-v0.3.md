# V0.3 实现与验证记录

日期：2026-09-07。对应 [简化技术方案](technical-design-v0.3.md)。V0.3 是方案版本，本次未执行版本发布、Git 提交或外部入口切换。

## 已实现

- Rust MCP Broker（固定 loopback `/mcp`）和 17 个静态工具，包含 4 个 Workspace、7 个 Serena Source、6 个内嵌只读 Git 工具。
- 本地添加/移除项目、官方命令创建及验证配置、激活/切换/取消激活、可选预索引及取消。项目配置使用官方 `Project.load` 路径验证，支持旧语言字段和 `project.local.yml`。
- 所有客户端共享活动项目；查询持共享锁，切换及初始化/索引持独占锁。已发布状态单独供 UI 展示，索引期间仍可显示真实活动项目并取消。
- 超时/取消、失效会话、项目外路径/junction 拒绝、UTF-8 输出限额和结构化错误结果；Git 禁用外部分页、外部 diff/textconv、fsmonitor 和可选索引锁。
- 官方 `serena-agent==1.7.0` 受管安装；拒绝旧版/不明版本。使用独立 `SERENA_HOME`、固定只读 context、空项目信任列表；固定 SSE 响应，使用 rmcp 3.2.0 默认 16 MiB SSE 消息上限，工具输出预算更小。
- 浅色侧栏首页、项目弹窗、独立服务卡片和 Broker 配置；保留已有设置、官方 Dashboard、托盘与单实例启动流程。

实际模块：`src-tauri/src/mcp/mod.rs` 集中 Workspace 协调与分派，`registry.rs` 定义公共 Schema，`server.rs` 管理 MCP HTTP，`serena.rs` 为具体 Adapter，`git.rs` 实现只读查询，`process.rs` 管理有界输出及命令清理。没有引入通用插件框架。

开发模式排除 `src-tauri/**` 的 Vite 文件监听，避免 Windows 编译产物锁定导致开发服务器 EBUSY 退出。重启开发服务器及 `cargo check` 后本机页面返回 HTTP 200。

直接新增依赖为 rmcp、tokio、tokio-util、axum 和 serde_yaml_ng；YAML 仅用于私有配置核验，不实现通用配置式 Schema 引擎。前端无新增 npm 依赖。

## 验证范围

| 验证 | 证据与边界 |
|---|---|
| Rust 单元/集成 | 最终 30 项通过、0 失败、0 跳过。通过真实官方 Serena 的创建、配置验证、七个 Source 工具、切换/取消、预索引及取消；Git 六工具、worktree、只读索引、选项拒绝及输出限额；HTTP 双客户端共享状态及排队取消 |
| 协调与配置 | 索引占独占锁时查询等待，UI 快照保留活动项目；旧 9120 配置可以加载；空信任列表强制生效；activation_command 测试标记文件未产生 |
| 前端 | TypeScript/Vite build 与 ESLint；Playwright 模拟 IPC 验证添加→初始化→激活顺序、取消激活、Broker 开关；预索引时复制及项目操作保持禁用 |
| 布局 | 1200×850 和最小 720×560，主区域可滚动且无横向溢出；弹窗可以完整操作 |
| Windows 二进制 | `npm run tauri build -- --ci --no-bundle` 生成 `src-tauri/target/release/serena-desktop.exe` |

真实 Serena 测试使用临时独立 Python 环境及临时 Git 仓库，不修改用户安装或业务源码。默认 `cargo test` 跳过需要外部环境的官方集成测试；本次显式设置 `SERENA_TEST_EXE` 并执行 `--include-ignored`，不把默认跳过当成通过。

可复验命令：

```powershell
npm run lint
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
$env:SERENA_TEST_EXE = 'C:/path/to/official/serena.exe'
cargo test --manifest-path src-tauri/Cargo.toml -- --include-ignored
npm run tauri build -- --ci --no-bundle
```

以下不纳入已通过结论：Cloudflare/ChatGPT 真实外部连接（未提供本次待验入口，未更改 Tunnel）；打包后 Windows 登录自启、托盘和手动唤醒的人机验收（保留已有实现，执行了相关自动化逻辑测试）。预索引失败状态由真实退出码及官方部分失败提示决定，不承诺所有语言服务都已安装或完成验收。

## 当前界面

下图为实际 React 界面的浏览器截图，使用模拟 IPC 数据。它用于布局与交互检查，不作为 Serena 后端运行证据。

![实现界面，模拟 IPC 数据](assets/serena-desktop-v0.3-implemented.png)

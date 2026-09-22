# Phase 3B macOS uv 与进程树设计

## 边界

本任务只完成 Phase 3 剩余的 P1 生命周期与安装链路，不混入 `/usr/bin/open`、Dock reopen、声音、LAN 权限或 DMG 发布。Windows 继续使用现有 Job Object / `taskkill` / `winget`，不为共享外观重写已验收实现。

## 进程所有权

新增私有 `macos_process` 模块，集中实现 Serena 与 cloudflared 共用的 Darwin 进程事实：

1. 在 `Command::pre_exec` 中调用 `setsid()`，消除 spawn 后再归组的竞态。
2. spawn 返回后读取 `proc_pidinfo + getpgid + getsid`，冻结 PID、PGID、SID 与启动令牌；只接受 `PID=PGID=SID`。
3. 信号发送前重新匹配完整 identity。`SIGTERM` grace 后只有 leader 仍匹配时才允许升级 `SIGKILL`；leader 消失但 group 非空时 fail-closed。
4. 完成条件同时要求直接 child 已回收、process group 为空。调用方在失败时继续持有 child/identity，以便重试或报告。

Serena 的同步 `std::process::Child` 与 Quick Tunnel 的异步 `tokio::process::Child` 保持各自 owner 类型，只复用身份观测与 signal/empty 判定，不增加跨平台 trait。

## uv 安装

macOS arm64 使用固定官方归档，不运行 `curl | sh`：

- 版本：`0.12.17`
- 资产：`uv-aarch64-apple-darwin.tar.gz`
- SHA-256：`85f00cbdc6dd3e97eba4c31b4d014375a9fdfe8f570023b84e5102fc3456896b`
- 归档成员：`uv-aarch64-apple-darwin/uv`
- 安装位置：`<runtime_directory>/uv/0.12.17/uv`

下载设置 120 秒超时和大小上限；先验证整个归档摘要，再只提取精确 regular-file 成员，设置 `0755`，通过同目录临时文件 `sync_all + persist` 原子替换。后续 Serena 安装仍使用现有固定 Python、包版本与隔离目录。

## 错误与验证

所有外部错误继续只返回稳定、无敏感输出的中文诊断。测试优先覆盖 fail-closed、组清理、非目标进程存活、uv 资产/摘要/归档路径；真实下载测试标记 ignored，常规测试不依赖网络。

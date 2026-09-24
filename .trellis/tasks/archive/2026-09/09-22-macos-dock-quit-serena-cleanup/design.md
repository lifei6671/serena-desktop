# 宿主退出子进程收口设计

## 范围与契约

本任务只处理 Serena Desktop 能够收到退出事件或正常执行清理代码的退出路径。宿主结束前，所有由现有模块 owner 启动并仍由应用持有 ownership 的外部进程都必须经过其正式 shutdown。`SIGKILL`、系统断电和其他完全无法执行应用代码的终止不在保证范围内。

不接管用户独立启动的同名进程，不通过进程名扫描或宽泛 PID 查杀推断 ownership。

## 根因

当前 `request_exit` 只由窗口、托盘、错误页以及 `RunEvent::ExitRequested` 调用。macOS Dock Quit 和 `Cmd+Q` 经过 Tao 的 `applicationWillTerminate`，直接投影为 `RunEvent::Exit`，因此跳过 `shutdown_impl`。Tauri 的 `App::run` 在事件循环结束后调用 `process::exit`，不能依赖 `ManagedProcess::Drop` 补偿。

## 方案

采用集中式、幂等退出协调：

1. 保留现有 `request_exit` 作为可取消退出入口；它隐藏窗口并运行正式 shutdown。
2. macOS `RunEvent::Exit` 增加最终同步收口。如果 shutdown 已成功完成则直接返回，否则在事件回调返回前执行同一正式 shutdown。
3. shutdown coordinator 只负责顺序协调现有 owner，不建立新的全局进程表。
4. 各 owner 的关闭结果独立收集；前一项失败不能阻止后一项执行。全部尝试后再返回聚合诊断。
5. `ShutdownState` 记录开始和完成状态，确保 `request_exit` 成功后由 `AppHandle::exit` 产生的最终 `RunEvent::Exit` 不会重复关闭。

## Owner 收口顺序

1. `SupervisorState::shutdown_capability_runtimes`：Serena/CodeGraph Workspace Capability Runtime。
2. `AgentProductService::shutdown`：Codex Agent Runtime。
3. `Broker::shutdown`：远程访问进程、MCP listener 及 workspace binding。
4. `SupervisorState::stop`：主 Serena MCP broker 及其 Process Group，其中包含 dashboard tray 子进程。

该顺序沿用当前 `shutdown_impl` 的依赖方向。区别是每一项都会执行，错误只在全部 owner 尝试后汇总。

## 失败语义

- 可取消退出入口发生清理失败时，不结束宿主，恢复窗口并记录聚合诊断，允许用户重试。
- macOS 已进入不可取消的 `RunEvent::Exit` 时，仍同步完成全部有界清理并记录失败；不得提前返回而跳过剩余 owner。
- 每个具体进程仍使用现有 Process Group、Job Object、温和终止、超时和强制终止逻辑；本任务不改变各 owner 的底层证据模型。

## 不采用的方案

- 全局 PID 注册表：重复现有 owner，容易误杀同名进程并破坏 Runtime identity。
- 自定义 macOS AppDelegate `applicationShouldTerminate`：侵入 Tao/Tauri 原生委托，维护成本和升级风险较高。
- 仅增加 `Drop`：`process::exit` 不保证运行析构器，无法解决 Dock Quit。
- 外部 watchdog：只为不可感知退出提供价值，超出用户确认范围。

## 验证

- 单元测试冻结 shutdown 阶段顺序、全部尝试、错误聚合和完成后不重复调用。
- 针对退出协调器执行 RED/GREEN，证明 macOS 最终 `Exit` 会补偿未完成 shutdown。
- 运行受影响 Rust 测试、完整 Rust 回归、格式检查及现有前端检查。
- 构建 `.app`，启动并确认受管进程存在；发送标准 Quit Apple Event；随后确认应用、Serena broker、dashboard tray、Capability/Codex/Remote 子进程均不存在，相关监听端口关闭。
- Windows 至少执行编译期条件检查；不修改其 Job Object 和既有退出入口。

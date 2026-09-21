# macOS 进程契约证据

## 官方与本机 SDK 证据

- Apple `setsid(2)`：成功调用会创建新 Session；调用进程同时成为 Session leader 和新 Process Group leader，返回的新 PGID 与调用进程 PID 相同。来源：<https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/setsid.2.html>
- Apple `killpg(2)`：向指定 Process Group 发送 signal；`ESRCH` 只表示当次未找到目标 group。来源：本机 macOS `man 2 killpg`。
- Apple `waitpid(2)`：父进程可按 PID 回收直接 child；Process Group 中的非直接后代不能仅靠 `waitpid` 证明已退出。来源：本机 macOS `man 2 waitpid`。
- macOS SDK `libproc.h` 提供 `proc_pidinfo` 与 `proc_listpgrppids`，声明至少从 macOS 10.5/10.7 可用。
- macOS SDK `sys/proc_info.h` 的 `proc_bsdinfo` 包含 `pbi_pid`、`pbi_pgid`、`pbi_start_tvsec` 与 `pbi_start_tvusec`，可作为私有 identity adapter 的候选内核事实。

本机 SDK 路径：

```text
/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/libproc.h
/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/sys/proc_info.h
```

## 设计结论

1. `setsid()` 之后仍在 child 和父进程两侧验证 `SID == PGID == PID`，不只依赖 API 成功返回。
2. `proc_pidinfo` 与 Darwin struct 只存在于 macOS 私有 adapter，不进入 Runtime 公共或持久化 contract。
3. `killpg(..., SIGTERM/SIGKILL)` 只是终止动作，不能单独作为终止证据。
4. 完整 live-host evidence 必须同时包含直接 child 已回收与 Process Group 为空的观测。
5. Process Group 不能阻止后代主动创建新 Session/Group，因此证据只覆盖仍属于原 containment 的进程。
6. 跨 Host/重启没有稳定 group handle；相关 migration、恢复与 Claim 规则必须在 Phase 2B 重新设计，并默认 fail-closed。

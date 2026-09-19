# P0-007 CodeGraph 多进程 Runtime 证据

日期：2026-09-19（Host / Windows）。CLI：`codegraph 1.6.0`。

## 隔离与准备

实验只使用本 task 下的临时根 `research/p0-007-temp/workspace-a` 与 `workspace-b`，以及 `%TEMP%/codegraph-p0-007-.../profile`。`HOME`、`USERPROFILE`、`APPDATA`、`LOCALAPPDATA`、`XDG_CONFIG_HOME`、`XDG_DATA_HOME` 均指向该 profile。先执行 `codegraph install --target=none --yes`，再分别执行 `codegraph init --yes <A|B>`。

两个 `status --json` 都为 `initialized:true`、`projectPath` 等于各自 canonical root、`pendingChanges` 三项均为零、`index.state:"complete"`、`index.reindexRecommended:false`。A/B 各只有自己的 Rust marker：`p0_007_marker_a` / `p0_007_marker_b`。

## 真实 server 与 MCP

CodeGraph 1.6.0 的默认 `serve --mcp` 会接入 detached shared daemon；这不能让 RuntimeSlot 的 stop 独占 server 生命周期。实验证明在隔离 profile 中设置 `CODEGRAPH_NO_DAEMON=1` 后，官方 `codegraph.cmd` 的同一 entrypoint（`node.exe --liftoff-only --disable-warning=ExperimentalWarning codegraph.js serve --mcp --path <canonicalRoot>`）以 direct mode 运行。每个实测 server 都是实际 `node.exe`，不是 `.cmd` wrapper，且 argv 从启动到停止固定带自己的 root。

| 项目 | A | B |
|---|---:|---:|
| 初始真实 server PID | 9420 | 41732 |
| 初始化到 MCP ready | 176.856 ms | 348.932 ms |
| steady Working Set | 87,658,496 bytes | 87,904,256 bytes |
| `tools/list` | `codegraph_explore` | `codegraph_explore` |

两个 server 并发 `tools/call(codegraph_explore)`：A 查询命中 A marker 且不含 B marker，B 查询命中 B marker 且不含 A marker（各 response 635 bytes）。因此 root 与 query 均未串线。

## 生命周期隔离

- 正常结束 A（stdin EOF）：9.580 ms，退出码 0；B 随后 query 仍只命中 B marker。
- A 再次启动为 PID 50456（identity 变化），query 仍只命中 A marker。
- 强制 `Stop-Process -Id 50456 -Force`：A 退出码 `4294967295`；B 随后 query 仍只命中 B marker。
- A 再建为 PID 53620（再次 identity 变化），query 仍只命中 A marker；A/B 的最终正常 stop 分别为 7.902 ms / 6.677 ms。

## 结论与策略输入

P0-007 **PASS**：A/B 双 server 同时存活、真实 process identity 和 root argv 独立、并发 query 隔离、stop/crash isolation、restart identity 变化均已实际验证。

- `maxInstances = 2`：仅依据以上双 server 并发证明；并非复制 Serena 值。
- `perSlotConcurrency = 1`：本 Work 没有同一 MCP server 的并发安全证据；且 CodeGraph 的单 SQLite query connection 会序列化 query。
- `idleTimeout = 300_000 ms`：server 冷启动约 0.17–0.35 s、正常 stop 约 0.007–0.010 s、每个 slot steady Working Set 约 84 MiB；首版以 5 分钟保守保温，避免短间隔请求反复冷启，同时有界地回收显著常驻内存。后续需要真实产品 workload 才可调整。

实现必须以 `CODEGRAPH_NO_DAEMON=1` 启动 provider-owned direct server；否则默认 detached daemon 会脱离 Slot 的 stop/crash ownership。

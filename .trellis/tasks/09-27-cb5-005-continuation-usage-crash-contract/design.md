# Stage A 技术设计

独立 Rust binary 使用 official agent-client-protocol =2.2.0（lock schema 1.9.1）。复用 CB5-004 SDK external-stream Tee/cleanup 模式，但新文件完全 task-local。initialize 保留冻结 shape（SDK UntypedMessage 扩展通道，typed response 校验），new/resume/load/prompt/permission 均官方 typed request。只有父 Host 可以运行 binary 的 resume/load 参数。

R1 与 R2 串行 child；同一个普通 temp workspace。每个阶段 complete manifest，异常不允许 PASS；child 回收失败不启动 R2。P1/P2 都不使用工具、不读写文件。UUIDv7 仅保留在进程内存与 ACP pipe，P2 prompt 不包含 token。

双向 Tee 总计上限 4 MiB，单帧 1 MiB，每方向最多 2048 frames；成功写入/读取才计 wire。SDK dispatcher 持续接收 early frames，按精确 RPC id 匹配响应。每 runtime timeout 120s；恢复 ACK 后 250ms replay 窗口，terminal 后 250ms late 窗口。所有 observed recovery updates 记录 type/order/count；250ms 不是 Provider replay completion 保证。P2 live answer 只拼接 exact session/current conversationRequestId 的 agent_message_chunk。未归因 live answer chunk 使结果 PARTIAL；late/replay 绝不重复计入结果。finalAnswerLength 是 UTF-8 字节数。

恢复响应在官方 schema 中不要求 sessionId；安全证据明确 sessionIdPresent。raw wire 若回显 wrong S1 或 early update wrong S1，则在 P2 之前 fail closed。响应 catalog 保存结构与字符串 hash/长度，不存任意标签、metadata、providerData。未支持 -32601 单独 UNSUPPORTED；其它失败 PARTIAL。无 production path 选择。

manifest 完整记录所有文件/目录，路径 hash，文件 size/hash；4096 entries/32 MiB 超限 fail closed。根与所有祖先的 reparse/symlink 均拒绝。不递归删除异常目录；只删除已经验证为空的 root。owned child kill/reap != Windows Job/tree containment，不涉及 Claim。

固定 env!(CARGO_MANIFEST_DIR)/../evidence/continuation；每 scenario sentinel create_new+sync_all，参数不允许改 output。父 Host 不应复制/重编译到新路径或删除 sentinel 重跑。失败保留 sentinel。

# CB5-005 Stage A — Continuation Contract

本轮用户收缩为 Stage A；只实现 resume/load 两个独立 Host scenario。Usage、Crash、schema proposal/DCR 全部不在本轮范围。任务保持 in_progress，停 Host Gate，不归档、不提交、不改生产源码。

## 验收

- direct installed CodeBuddy 2.158.0 / ACP v1，标准 Host 环境，ordinary Win32 fresh temp cwd。Agent 不运行真实 CLI。
- 每个 scenario 独立 sentinel/root/session；R1 initialize/new/read-only P1 terminal/reap；R2 fresh initialize/指定 typed recovery exact S1 与同 cwd/read-only P2 terminal/reap。失败不 fallback。
- 使用 official agent-client-protocol 2.2.0 typed ResumeSessionRequest / LoadSessionRequest，mcpServers 按 SDK 实际序列化，不猜测字段。
- P1 随机 UUIDv7 memory token 仅 conversation；P2 不重发 token。证据仅 tokenSha256、matched、finalAnswerSha256/UTF-8 length，无 prompt/token/正文。
- exact S1、safe recovery params、实际 response sessionId/catalog、replay update types/count/order、P2 prompt/terminal identity、全量 workspace delta。
- failed recovery 禁 P2；ACK 但 token 不匹配只能 PARTIAL；load 与 resume 的 replay 均据实捕获并隔离 live。
- evidence/continuation 固定路径，二进制内 create_new + fsync sentinel，无 output/force/retry 参数。
- manifest 包含隐藏项，symlink/reparse fail closed；有界 frame/queue/timeout/cleanup，RPC id correlation；不留 stderr/env/credential/agent thought。
- deterministic tests、固定 Host 命令、review target 与独立只读 review；真实 scenario 都 NOT_RUN。

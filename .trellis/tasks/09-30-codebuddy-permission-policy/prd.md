# CodeBuddy 本地自动 Permission Policy

Host 已明确授权直接实现。保持 ACP auto，处理后续 request_permission，仅选择 advertised 单次 AllowOnce/RejectOnce，不调用 AI，不使用 bypass，不改变 Codex 或进程 containment，不提交 Git。

验收：冻结 execution workspace/mode；有界 exact tool snapshot；workspace 路径与开发命令矩阵；allow 不产生 denied 投影；保留拒绝 terminal/release 合约；focused tests、fmt、check；记录 Windows 和重启后真实 E2E 未运行。

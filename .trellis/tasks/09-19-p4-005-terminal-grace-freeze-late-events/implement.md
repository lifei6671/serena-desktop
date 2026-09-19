# P4-005 实施计划

1. 审计 P4-004 Usage store/projector、runtime termination transaction 和 Codex provider terminal path。
2. 在 store 加入 grace/freeze 与 projector state/deadline gates；把 runtime teardown freeze 纳入既有 authoritative transaction；编写确定性 tests。
3. 在 provider 保持 terminal/recover/cleanup/finish 顺序后添加有界 exact-Usage drain 与 best-effort freeze；用 paused Tokio time/fake transport 测试。
4. 运行指定 focused suites、format/check/diff check，记录 evidence；冻结目标后执行独立只读 review，修复 P0/P1 后复验复审。

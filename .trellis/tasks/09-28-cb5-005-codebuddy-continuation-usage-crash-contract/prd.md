# CB5-005 — Continuation / Usage / Crash Contract Freeze

## 目标

使用本机已安装的 CodeBuddy Code 2.158.0、真实 `node.exe + codebuddy --acp`、标准用户环境和 ordinary Win32 临时目录，冻结：

1. `Continue` 是 `PROVEN_SUPPORTED`、`EXPLICITLY_UNSUPPORTED` 还是 `INCONCLUSIVE`；
2. 真实 `usage_update` 的逐字段形态、顺序、scope、restart/reset、terminal coverage 与 late 行为；
3. 四个 crash/restart 窗口能恢复 session、exact target Prompt result 或完全不能恢复；
4. 后续实现所需的最小 Provider-private persistence 字段以及是否需要新 schema。

## 权威与冻结基线

- `HEAD=b83418a3d12681d1cb372eebd9f58c53e08cc98a`
- `.trellis/tasks/09-26-cb5-004-cancel-permission-contract/evidence/host-cancel-permission-proof/acceptance.json` SHA256 `85e796bd3c74bfc4e839772fd2617f3e65a6c0690378830d74bea831217b6c9d`
- `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` SHA256 `7af023d5e5a84e3106b12db14ac44eb9b4ba6e003cd3fdc2027c2fe29176f334`
- `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` SHA256 `a80a5a6fa0b3d263b194f7d4604933a805475ca15761eeae86c3c7029e0e6e15`
- CB5-004 direct Host wire SHA256 `bf193e9d353357a1114293307107a4d5b6a1e671e6b2a6cb4c4fe45ae04b1b01`
- CB5-004 exact-launcher Host wire SHA256 `5f2c4df063d214627f17e8a5bf77975d6b4e935d648a2127d0b2d16ec81b9e9f`

## 验收

- 静态 inspection 先确定唯一 recovery method；真实 Probe 不 fallback、不盲试第二方法。
- 真实 prompt 使用固定无敏感 fixture；permission 只允许 typed `reject_once`，从不自动 allow。
- 每次真实 Probe 有 bounded timeout、sanitized raw wire、PID/exit/cleanup、workspace before/after manifest/hash。
- crash 四窗口分别为：session/new 后 prompt 前、prompt write callback 后 terminal 前、marker side effect 后 terminal 前、terminal 后 summary persist 前。
- R2 证据不当作 R1 Runtime/Job termination 或 Claim release 证据。
- task-local unit tests 必须先 PASS；fake/fixture 只验证 parser/sequence/normalization，不能替代真实 wire。
- initialize fixture 必须直接证明 harness params 与冻结 CB5-004 Host PASS wire 逐字段相等；旧非等价 attempt 只作 diagnostic history。
- fresh session prerequisite 失败时必须停止该 attempt 的后续 prompt/recovery/crash 调用。
- 最终 Contract authority 必须来自 SerenaDesktop command Host environment；Agent-shell `process.env` Probe 只能是 diagnostic。
- Host attempt-4 必须显式门禁、独立 evidence root，并保存 allowlist-only environment provenance。
- 最终独立只读 `FULL_SCOPE` review，生产源码零 diff；不 commit/push，不进入 CB8-003/CB8-004/Usage implementation。

## 非目标

- 不修改 `src-tauri/src/agent/codebuddy`、capabilities、Provider Registry、StateStore schema、Product DTO 或 UI。
- 不实现 migration，不修改技术方案正文，不模拟 continuation，不 replay parent prompt，不 fallback 到 Codex。

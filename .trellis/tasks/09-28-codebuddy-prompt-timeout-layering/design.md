# 设计

## Timeout authority

`Limits` 是 CodeBuddy 私有 Runtime 的既有集中上限 authority。新增 `prompt_timeout: Duration`，生产默认 1 小时；保留 `request_timeout: Duration` 的 15 秒默认及所有既有控制边界消费者。

`Requests::request()` 已通过 SDK `JsonRpcRequest::to_untyped_message()` 精确识别 `session/prompt`。在 enqueue 前选定本次 request timeout：Prompt 使用 `prompt_timeout`，其余 request 使用 `request_timeout`。超时分支继续调用 `Shared::fail(Failure::Timeout)`，因此 fail-closed、Uncertain 与 recovery 链路不变。

## Cancel contract

Prompt timeout 只约束尚未返回的原始 Prompt request。`Requests::cancel_session()` 的 physical flush timeout、`prompt.rs` 从 cancel flush 时刻计算的 terminal deadline，以及 permission/final flush 上限继续使用 `request_timeout`。

原 request timeout 分支保留既有 `cancel.used` 判断：若 Prompt cancel 已开始，该分支不再从 Prompt 起点宣告超时；cancel send/flush 或 owner 的绝对 deadline 会先通过共享首错唤醒 Prompt waiter。长 `prompt_timeout` 不参与取消 deadline。

## 测试设计

- client fake peer + Tokio paused time：非 Prompt 在短 `request_timeout` 到期后失败。
- client fake peer + Tokio paused time：Prompt 跨过短 `request_timeout` 仍 pending，并可收到正常 response。
- client fake peer + Tokio paused time：Prompt 到达独立 `prompt_timeout` 后失败。
- 既有 native cancel 集成测试显式配置短 `request_timeout` 与长 `prompt_timeout`，并由外层短完成上限证明取消不等待 Prompt timeout。
- 既有 Prompt timeout/failure 测试显式使用短 `prompt_timeout`，继续验证 Uncertain/recovery 契约。

## 风险边界

- `Limits` 是 crate-private 测试可注入结构，不产生公共配置兼容问题。
- 生产默认 1 小时覆盖几十分钟 Agent 推理，同时保持有界；未发现仓库内另一项更具体的 CodeBuddy Prompt duration authority。
- 不触碰 Windows script-path 未提交改动。

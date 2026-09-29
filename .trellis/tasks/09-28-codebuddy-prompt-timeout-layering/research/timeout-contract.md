# CodeBuddy Prompt timeout contract

## 当前事实

- `Limits::default().request_timeout` 为 15 秒。
- `Requests::request()` 能精确识别 `session/prompt`，但当前所有 request 共用 `request_timeout`。
- cancel 开始后，原 Prompt timeout 分支会 pending；`cancel_session()` physical flush 与 `prompt.rs` cancel terminal deadline 使用 `request_timeout`。
- `prompt.rs` 的 Prompt physical flush、permission publication 与 final flush 也使用 `request_timeout`，属于短控制边界。

## 裁决

- 新增私有 `prompt_timeout`，生产默认 1 小时。
- 只改变 `Requests::request()` 对原始 Prompt response 的总等待上限。
- 所有短控制边界继续使用 15 秒 `request_timeout`。
- Prompt timeout 保留 `Failure::Timeout -> Shared::fail -> Uncertain/recovery` 的既有失败路径。

## 基线隔离

任务开始前已有 `windows_launcher.rs`、`windows_launcher/tests.rs` 和 `.trellis/tasks/09-28-codebuddy-windows-script-path/` 改动；它们不属于本卡，不修改、不评审、不归因。

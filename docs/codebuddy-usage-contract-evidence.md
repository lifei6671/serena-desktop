# CodeBuddy Usage：正式受管 SDK Contract 与 direct writer（2026-10-01）

**本 Work 仍 BLOCKED：三轮真实 typed PromptResponse.usage 都是 None。** 公共 direct writer 已完成；没有接入 CodeBuddy Usage mapper，也没有开启 token_usage。此文取代上一轮 raw Python Probe 结论；旧 probe.py / observations.json 已删除，不能用其 session/new 失败作为 Contract Gate。

## 正式 Contract Test

在 `src-tauri/` 执行：

```sh
cargo test --lib real_codebuddy_managed_end_turn_usage_contract -- --ignored --nocapture --test-threads=1
```

测试位置：`src/agent/codebuddy/provider/macos_tests.rs`。真实 binary 由生产 `discover()` 解析，执行 `--version` 断言 **2.160.0**。使用生产 LaunchRequest、Runtime::start、官方 SDK typed NewSessionRequest / PromptRequest / LoadSessionRequest，以及生产 new_conversation_id()。exclusive Runtime / Session 的每个 Prompt 独立 conversationId，response `_meta.codebuddy.ai/conversationRequestId` 必须精确匹配；没有用时间窗口归因。

每个 Runtime 的 RPC 序列有 180 秒 deadline，沿用生产 transport/queue 上限，持续 drain session queue，避免长模型请求触发队列 TTL；成功、timeout 或 assertion failure 后均先调用 Runtime.shutdown() 并确认 process group cleanup，最后才汇报合同失败。Prompt 要求只回复短文本且不使用工具；测试不输出 prompt、result、wire、日志、凭据，仅输出 Session / conversation identity 和 typed Usage 数字或 None。

本轮最近一次完整三轮执行观测（两个 Runtime 的 cleanup 均通过）：

| 场景 | Runtime | exact Session | conversationRequestId | stop | typed usage |
|---|---|---|---|---|---|
| Fresh P1 | R0 | 01a0f50b-63c8-776e-9ceb-746dacb42d3a | 01a0f50b64097447a7283868ea67a4ef | EndTurn | None |
| 同 Session P2 | R0 | 同 S1 | 01a0f50b8d3b7881b8df07e3983199a3 | EndTurn | None |
| 重启 session/load 后 P3 | R1 | 同 S1 | 01a0f50bae2d7b9aa9d041352bfa2198 | EndTurn | None |

三次 response conversation 精确匹配。完整运行耗时 28.78 秒，最后 gate assertion 明确报 `BLOCKED: CodeBuddy 2.160.0 PromptResponse.usage absent`，测试结果 1 FAILED，不能报告 Contract PASS。上表是 typed Option 的结果，**不是原始 wire 对 usage 属性的存在性证明**：SDK 对该可选字段使用 DefaultOnError，缺失、null 或无效形状都可能解析为 None。三轮没有任何可供公共 writer 消费的有效 typed Usage，不能猜 per-turn/cumulative。

开发探针期间出现过 session/new Remote、使用非生产格式 conversationId 后的 Prompt Remote，以及未持续 drain 时的 QueueExpired；均未作为 Usage Contract 结论。

完整三轮采集后，测试补充了与生产一致的 take_session_new_extensions / take_session_load_extensions 身份校验。最后一次复验及一次复试均在 session/new 返回 Remote（分别 1.16 / 1.01 秒），都在报告失败前完成 Runtime.shutdown。根因未定位，不能把这两次失败当作 Usage unsupported，也不能声称最终测试可稳定复验；此前完整三轮的有效 typed usage=None 记录仍如上。当前 Work 同时受缺少有效 Usage 与 Host session/new 复验失败限制。

## 固定 SDK/schema

普通 `[dependencies]` 保持 `agent-client-protocol = =2.2.0, default-features=false`，不启用实验 feature。仅同包 `[dev-dependencies]` 显式声明 `unstable_end_turn_token_usage`，利用 Cargo resolver 的 dev feature union，让测试构建可读取 typed `PromptResponse.usage`。该 feature 仅服务测试 Contract Probe，不进入普通生产 check/build 的 feature surface；包含 dev targets 的构建（例如测试或 `--all-targets`）会启用它。

实际依赖 schema 为 **1.9.1**；没有修改 Cargo.lock。下述真实采集来自开启 dev feature 的测试构建，不能据此宣称生产启用了 end-turn Usage。

本机 registry `agent-client-protocol-schema-1.9.1/src/v1/agent.rs:3114`：

```text
schema::v1::PromptResponse.usage: Option<schema::v1::Usage>
Usage.total_tokens / input_tokens / output_tokens: u64
Usage.thought_tokens / cached_read_tokens / cached_write_tokens: Option<u64>
Usage.meta: Option<Meta>
```

PromptResponse.usage 注释声明 for this turn；Usage 的部分字段注释同时出现 across session / all turns。SDK feature 只能暴露解析能力，不能要求 Provider 返回字段。此次三轮均无有效 typed Usage，因此不冻结 CodeBuddy per-turn 语义，不编写基于假设的 mapper。

stable UsageUpdate 的 schema (`src/v1/client.rs:609`) 明确 used=u64 为当前 context occupancy，size=u64 为 context window，cost 为 optional cumulative session cost (`amount:f64`, `currency:String`)。这些不是已绑定当前 Prompt 的消费 total；本次不使用 usage_update / context occupancy / cost 作为正式统计 Authority，也不读取 ~/.codebuddy 日志或消息正文。

## 已完成公共 writer 架构修正

- `UsageEvent` 明确分为 `Cumulative(CumulativeUsageEvent)` / `Direct(UsageSnapshot)`，保留现有 cumulative constructor，新增 direct constructor。Projector 依据 typed variant 路由，没有 providerId 字符串分支。
- Codex 继续使用原 `project_execution_usage` 的 baseline/epoch/grace 逻辑；该入口拒绝 Direct variant，未改弱旧计数语义。
- 新增 `StateStore::project_direct_execution_usage`：读取 Execution 并核验 Provider，拒绝覆盖其他 Provider 的已有 usage 行；只写 public execution_usage，不访问两张 Codex private 表。
- 入口接收公共 i64-domain UsageSnapshot，并复用严格公共数值校验：负数拒绝，null/0 保持区别，不计算 total。Provider 的未来 u64 mapper 仍需 checked conversion，当前 CodeBuddy 未接入任何 conversion。
- Store 拥有 usage_revision；字段与 completeness 相同是 semantic no-op（updated_at / event revision 变化也不更新）。complete 行持久冻结，重复为 no-op，不同 late final 被拒绝；重启后规则仍成立。
- 只改变 public Usage，Execution 全行及 Activity/Claim/lifecycle 不由 writer 改动。projection failure 仍被 TelemetryProjector 丢弃，不返回 Provider terminal failure。

新增 Store / Projector 测试覆盖 Provider mismatch、foreign persisted Provider、null/zero/full breakdown、total 不由 breakdown 推导、重复不更新 revision/timestamp、负数拒绝、restart/frozen late、Codex private 表不产生记录、Execution 无副作用与 sink fail-safe。

## 当前产品语义与限制

Fresh / Continue / load 后新 Execution 继续 unknown/null；没有父 Execution Usage 加入 child，也没有 baseline/subtract。terminal 不代表 Usage complete。没有 CodeBuddy 数字样本、字段转换或 complete 覆盖证明，因此 CodeBuddy capabilities().token_usage 继续 false（所有平台）。真实 CodeBuddy mapper/overflow/exact conversation 发布与 capability=true 的实现测试尚未存在，不能报告这部分已实现。

## 最终验证

在 src-tauri 下：

- `cargo test --lib usage -- --test-threads=1`：60 PASS，1 ignored（真实 contract 单独运行）；包含原 Codex private Usage 与 CB9 Product regression。
- `cargo test --lib agent::codebuddy -- --test-threads=1`：122 PASS，3 ignored，涵盖现有 ACP/macOS/permission/provider 测试。
- 真实 end-turn Contract 命令：一次完整三轮均成功 EndTurn，但 usage=None，gate 1 FAILED / BLOCKED；最新两次复验在 session/new Remote，分别 1 FAILED，cleanup 均成功。
- `cargo fmt --all -- --check`：PASS。
- `cargo check`：PASS。
- `cargo clippy --lib -- -D warnings`：FAIL，8 项既有 dirty 问题：permission.rs:126/129/316，recovery.rs:97，permission_policy.rs:104/434，codex/macos_recovery.rs:185/245；没有本次新增文件位置的 lint，未整理旧改动。

仓库根目录：`node --test src/AgentPanel.test.mjs src/App.test.mjs src/agentContract.test.mjs`：127 PASS；`git diff --check`：PASS。

保留现有 dirty worktree；没有 reset/checkout/stash、commit/push。后续只有获得 fixed Provider 有效 end-turn Usage 或另一个已证明精确 identity 与 counter 语义的 Usage Authority 后，才能完成 CodeBuddy 数字映射并开放 capability。

## Host Review 修正：实验 feature 仅供测试

本次仅调整 Cargo 声明和本文，不修改 Direct writer、TelemetryProjector、CodeBuddy capability，也不新增 mapper。

- 普通 dependencies：`agent-client-protocol = { version = "=2.2.0", default-features = false }`。
- dev-dependencies：同版本、default-features=false，额外启用 unstable_end_turn_token_usage。Edition 2024 的默认 Cargo resolver 将未参与普通构建的 dev feature 与生产依赖隔离；测试 target 通过合法的同包 feature union 启用它。
- `cargo tree --manifest-path src-tauri/Cargo.toml -e normal,build,features -i agent-client-protocol-schema`：没有 unstable feature。
- 同命令改为 `-e normal,build,dev,features`：unstable feature 来源明确标记为 `[dev-dependencies]`。
- 生产 `cargo check --manifest-path src-tauri/Cargo.toml`（另用 -vv 检查实际依赖 artifact）：PASS；该 check 引用 ACP artifact `b7d5da9e9008e556`，其 Cargo fingerprint features 为 `[]`。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib usage -- --test-threads=1`：60 PASS、1 ignored。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib real_codebuddy_managed_end_turn_usage_contract --no-run`：PASS，typed usage 字段可在测试构建读取；本次没有实际执行 Provider Probe。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`git diff --check`：PASS。

“dev feature” 指包含 dev-dependencies 的 target，不能与 debug/dev profile 混淆：普通 debug check/build 仍没有该 feature；测试及包含 dev targets 的 --all-targets 构建会启用它。token_usage 继续 false，未提交或推送。

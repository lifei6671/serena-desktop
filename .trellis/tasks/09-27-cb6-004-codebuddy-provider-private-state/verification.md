# CB6-004 验证记录

同一 delivery unit，基线与 task 不变。上一执行被 Host cancel，尚无测试 PASS。
Host correction 已纳入当前 design/schema/domain/SQL：prepared 永不允许 null conversation；create 原子 UUIDv7 reservation；ready provenance 可后补；MarkSent 必须全部齐；仅 exact live terminal 可 uncertain -> terminal_observed；terminal 身份冻结。
历史草稿保留 design-draft-history.md，不能用其中 pre-prepared 解释作为当前 authority。
check-first.log: FAILED，旧草稿 SDK root import 错误；改为 SDK schema namespace 后重跑。
验证待执行：focused store/migration、frozen 历史、fmt/check/clippy、scope/hash、独立只读 full review。

## 2026-09-27 closeout WorkRun

- 新 Thread 01a0e201-7afd-7b00-9fd7-25dd3712f728，继续同一 delivery unit/task；原始 HEAD b40d67b392669a087dcd4d6d4e59065c233f292b。
- closeout 接手 dirty 实现来自前序已审读工作，属于本卡 delivery-owned 内容；不回退/清理，不重新设计。
- 补齐 focused migration/private store/public projection tests。生产修改仅允许本卡编译/lint/测试或评审证实的必要修复。
- 所有 Cargo 命令 cwd=src-tauri，native Windows。Linux NOT_RUN：项目没有 Docker runner；不使用 WSL，不把 Windows 结果称为 Linux 证据。
- 验证顺序：fmt check → check --lib --tests → v13 migration → private store → agent::store::tests → public projection → clippy --lib --tests -- -D warnings → diff/scope/v12 hash。
- 独立只读 full review 在 executable hash manifest 冻结后进行；P0/P1/P2 修复后重新冻结/评审。
- 无 commit/push；无 CB6-005、网络 session/prompt、usage ledger、acceptance/Claim/termination authority 或 capability/public DTO 变化。

### 编译阶段记录

- closeout-fmt-initial.log：FAIL exit 1，本卡 inherited/new Rust 格式差异；仅格式化本卡 Rust 文件后修复。
- closeout-check-initial.log：FAIL exit 101，新增 v13 hash 测试使用 LowerHex，但 sha2 0.11 digest 不实现该 trait；改为逐字节 hex，不改 schema 或依赖。
- closeout-fmt.log：PASS exit 0，cargo fmt --all -- --check。
- closeout-check.log：PASS exit 0，cargo check --lib --tests。
- 前序 check-first.log 与 check-corrected.log 保留：均为旧 SDK import path 失败，不代表当前状态。

### 测试修复历史

- closeout-v13-initial.log：FAIL exit 101，5 passed / 4 failed；第一条为内存 schema fixture 调用磁盘 configure 的 `WAL unavailable: memory`。内存夹具改为 foreign_keys=ON；真实磁盘 WAL/reopen 检查不变。closeout-v13.log：PASS 9/9。
- closeout-private.log：PASS exit 0，9/9（typed private store）。
- closeout-store-initial.log：FAIL exit 101，60 passed / 1 failed；store/tests/work_runs.rs:62 遗留当前版本断言 12，实际为 13。仅更新此断言为 13，未弱化任何历史值断言。
- 修复后 fmt/check 再次 PASS；全量 affected store 重跑进行中。

### 最终已执行测试（native Windows）

所有 Cargo 命令 cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop\src-tauri`；`cargo` 为 `C:\Users\lifei\.cargo\bin\cargo.exe`，版本 1.98.0。

| 命令 | 结果 | 日志 |
|---|---|---|
| cargo fmt --all -- --check | PASS exit 0 | closeout-fmt.log |
| cargo check --lib --tests | PASS exit 0 | closeout-check.log |
| cargo test --lib agent::store::tests::v13_migration -- --nocapture | PASS exit 0; 9/9 | closeout-v13.log |
| cargo test --lib agent::store::tests::codebuddy -- --nocapture | PASS exit 0; 9/9 | closeout-private.log |
| cargo test --lib agent::store::tests -- --nocapture | PASS exit 0; 61/61，包含上述18项 | closeout-store.log |
| cargo test --lib codebuddy_private_state_is_absent_from_public_projections -- --nocapture | PASS exit 0; 1/1 | closeout-projection.log |
| cargo test --lib agent::provider::tests -- --nocapture | PASS exit 0; 10/10 | closeout-provider.log |
| git diff --check（repo root） | PASS exit 0 | closeout-diff-check.log |

新增未跟踪 source/fixture 另逐行扫描：0 trailing whitespace。测试 linker stdout 为 MSVC 创建 import library/object 提示，命令最终 exit 0；不将它当作测试失败或 Linux 证据。

v12 原始 SHA256：CE03BD693CE91308A004CDB2FF84AD280F44F25041CDEC6D66599B34A19B65BE，与 baseline.json/closeout-baseline.json 完全一致；git diff --exit-code HEAD -- src-tauri/src/agent/schema_v12.sql exit 0。

### Clippy baseline 与 scope Gate

`cargo clippy --lib --tests -- -D warnings`：FAIL exit 101，仅 1 条既有 baseline：src-tauri/src/agent/store/usage_tests.rs:987 `clippy::await_holding_lock`，await 位于 1012/1016。closeout-clippy.log 保留原始输出。该文件 `git diff --exit-code HEAD -- ...` exit 0，HEAD 中相同行确实存在。没有本卡 lint、没有 suppress lint、没有修改 usage 文件。按本轮用户明确约定，单独记录该 baseline 作为 closeout 例外；原始 Clippy 结果仍为 FAIL，不能宣称全仓 Clippy clean。

Scope：14 个 executable source/test/fixture 文件，均属于 CB6-004；另有同一 task 的设计/验证/日志。store/tests/work_runs.rs 仅当前 schema 版本断言 12→13。无 staged changes、无 commit/push、无新 task、无 schema_v12 改写、无依赖/lockfile/public DTO/capability 变更。完整文件 hash 见 executable-target.json。

### Closeout 结果

2026-09-27：独立 CHILD_AGENT / FULL_SCOPE review PASSED；14/14 文件完整覆盖，FRESH，无 P0/P1/P2，无 freeze 后修复轮次。见 review.md / executable-target.json。主代理复核 frozen source hashes、HEAD、schema_v12 原始 hash 全部一致。

CB6-004 task completed（遵守用户允许精确记录的唯一 Clippy baseline 例外）。Required task tests/check/fmt/scope/hash/review Gate 满足；不宣称全仓 Clippy clean 或 Linux PASS。保留所有初始失败与修复轨迹，没有 commit/push，没有推进 CB6-005。

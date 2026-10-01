# Stage B Host Freeze verification

当前：**Stage B Host Gate PASS**。详见 [Host Freeze](usage-host-freeze.md) 与 [只读检查](usage-host-freeze-checks.json)。

- Host 唯一 usage-repair：command-26872-1790480352549345-80，timeoutMs=420000，completed / exitCode=0 / timedOut=false；result scenario=usage-repair,status=PASS。
- 首次 usage：command-26872-1790479449915847-74，默认30s外层 COMMAND_TIMEOUT；[原 timeout evidence](evidence/usage/usage.host-timeout.json) 保留。它不是 Provider Usage 结果；原 sentinel 永不 replay。repair sentinel 也永久保留，不再执行真实 usage/resume/load。
- 本轮只读复核 pinned 2.158.0 bundle 与 raw evidence，数值、ordering、meta key hashes、terminal projection 检查 PASS；raw evidence 不改写。
- 本轮仅文档/状态变更，测试/build/fmt 不重跑；历史 harness gate 见下文和 [repair verification](usage-repair-verification.md)。本轮文档链接/JSON/作用域 hash 检查及 git diff --check 见新增 review 记录。
- token_usage 建议 supported but completeness-aware；生产实现未验收，Crash NOT_RUN，task 仍 in_progress。

## 历史：Stage B harness 交付时验证（当时真实 usage NOT_RUN）

# Stage B verification

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。TASK为当前task，Cargo/cache/source参数完整命令见usage-README.md。

- PASS exit0 — `cargo test --offline --locked --manifest-path TASK/harness/Cargo.toml usage_`：14 passed、0 failed、16 Stage A tests filtered out，1.39s。
- PASS exit0 — `cargo build --offline --locked --manifest-path TASK/harness/Cargo.toml`：Windows binary已生成；最终注释同步后再次构建。
- PASS exit0 — `cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check`。
- PASS exit0 — `git diff --check`。
- PASS — task.py current --source：本任务仍为当前in_progress。
- NOT_RUN — 真实CodeBuddy usage；停父Host Gate。Stage A真实resume/load没有重跑或改写。

Typed schema来源为既有官方ACP 2.2.0/锁定schema1.9.1本地cache；没有新增依赖。仅Windows验证，无WSL/Linux PASS声明。

作用域见usage-scope.json。新增usage模块/fake/tests及本轮文档；main只增加usage入口/注释/参数；旧参数测试用crash代替现在合法的usage。Stage A实现和真实证据原字节保留。task.json仅同步当前Stage B描述。

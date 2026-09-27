# Stage C verification

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。命令全参数见 [crash-README.md](crash-README.md)，所有Cargo命令都指向task-local harness/Cargo.toml，offline/locked，不使用主Cargo。

- PASS exit0：task-local `cargo test`，45 passed / 0 failed / 0 ignored，2.31s（11新增crash + 34既有fake/unit回归）。
- PASS exit0：最终Stage C诊断/错误响应边界修改后 `cargo test ... crash_`，11 passed / 0 failed / 34 filtered，1.44s。
- PASS exit0：task-local `cargo build`，最终dev binary，1.99s。
- PASS exit0：task-local `cargo fmt -- --check`。
- PASS exit0：`git diff --check`；task-local未跟踪新增文件另做空白/冲突标记检查。
- PASS exit0：父 Host 真实 `crash-before-terminal`，CommandRun `command-26872-1790483578322192-95` completed；result 为 `PASS` / `partial`，`workspaceDelta=[]`、`workspaceDeleted=true`。
- PASS exit0：父 Host 真实 `crash-after-terminal`，CommandRun `command-26872-1790483608562452-96` completed；result 为 `PASS` / `partial`，`terminalStopReason=end_turn`、exact conversation identity、messageId set 与 live answer 均匹配、`materialContractDifference=false`，`workspaceDelta=[]`、`workspaceDeleted=true`。
- PASS exit0：本轮只读 Host freeze 检查；两份 result JSON 可解析，语义断言与 SHA256 匹配，六份原始 evidence 原字节保持。未运行任何真实 mode、binary、测试或 CodeBuddy CLI。

覆盖：trigger correlation/terminal优先、RPC error不冒充terminal、terminal-first不重试、wrong Session/request/message、分组去重/hash、无terminal最高partial、future terminal candidate只标差异、load失败不fallback/R2无prompt、write-ahead identity先于wire、fresh cwd exact、cleanup失败/timeout/manifest bytes/hidden、durable sentinel不重放、安全flag恒false。既有回归补充frame count、junction/reparse、kill失败仍wait、manifest/cwd边界。

主Agent只运行Python fake child及测试，不启动真实CodeBuddy。Windows验证，不使用WSL，无Linux PASS声明。Stage A/B raw evidence与源码模块保护由crash-baseline.json/scope记录验证；main只增加Stage C入口、模块注册和参数名。

schema只读核对store.rs当前user_version=12和schema_v9/v12。DCR提出next version（当前候选v13），不创建migration，不访问真实DB。生产源码、主Cargo、现有DB schema均未修改。

独立只读 review 的 pre-Host 最终 target/freshness 及 P0/P1 见 [crash-review.md](crash-review.md)。后续 Stage C Host freeze 见 [crash-host-freeze.md](crash-host-freeze.md) 与 [crash-host-freeze-checks.json](crash-host-freeze-checks.json)。Stage A/B/C Host Gate 均 PASS，CB5-005 Contract Freeze 可以完成；Crash result recovery 仍最多 partial，不宣称生产 Runtime/Job/StateStore 已实现，也不授权 Claim release。

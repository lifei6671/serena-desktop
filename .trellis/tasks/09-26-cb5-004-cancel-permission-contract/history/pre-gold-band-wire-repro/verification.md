# CB5-004 canonical session/new repair verification

结果：**C repair FAIL（HTTP_500）；本轮 cancel-before / cancel-after / permission-deny 全部 NOT_RUN，因 C Gate 未通过。** 停止等待 Host Gate。上一轮三场景 PARTIAL 与 7 次记录保持独立，没有把 fake PASS 作为真实能力验收。

## 本轮唯一真实调用

- argv：`C:\nvm4w\nodejs\node.exe`、`C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy`、`--acp`；无 permission-mode/tools/settings flags。
- cwd：`C:\Users\lifei\AppData\Local\Temp\cb5-004-5S5Mf4`，fresh harness-owned 临时目录。
- initialize：成功，protocolVersion=1。
- session/new request sequence=3，RPC id=`c14687a6-29ff-459b-a088-99060f538d10`，实际完整 params 为 `{cwd: 上述路径, mcpServers: []}`。
- 精确同 RPC id response sequence=4：JSON-RPC -32603，静态错误分类 HTTP_500；没有 sessionId/modes/configOptions。
- 没有 session/set_mode、session/set_config_option、prompt、cancel 或 permission 请求；无 Provider terminal。
- before={}、after={}、delta=[]；direct Child reaped、stderr joined、workspaceDeleted=true；harness exit0 表示清理成功，runner exit1 表示 C Gate 失败。
- 没有追加、自动重试或未知副作用重放。本轮真实进程共1，三场景各0。

证据在 `evidence/canonical-repair/session-new-repair-result.json` 与 `session-new-repair.jsonl`。sanitizedRawLine 是 Tee 实际成功收发帧经明确白名单脱敏后的 NDJSON；session/new cwd/mcpServers/id 保留，错误任意文本不落盘，分类不是 Provider terminal。RPC exchange 按 ID 匹配，不能拿下一帧作为响应。

本次证据排除了“仅移除启动参数即可恢复”的充分性；**不能**证明 flags 从无影响，也未定位服务根因。没有读取用户 settings/凭据或扩展排查。

## 受控修复

所有真实 launch 统一 canonical `--acp`。显式 `NewSessionRequest::new(cwd).mcp_servers(vec![])`。成功响应缺空 sessionId 也 fail closed。

before 使用默认 session 配置；after 只有本次成功 NewSessionResponse 的 typed modes 目录实际广告 auto 时，才使用该广告 ID 构造官方 `SetSessionModeRequest`，等待 ACK 后才发 prompt。没有提前 CLI 注入，也没有猜测 mode/config/provider ID。缺目录或 new failed/unknown 就不发送 mode/prompt。permission 用默认 session 和安全 temp-only Write 请求，不加 tools/settings。C 未通过，本轮这些场景路径只通过 fake/unit 验证。

保留原 cancel/permission 合同：exact runtime/session/prompt 身份、UUIDv7 simple 发送前 durable；before 真实相关 update+零 manifest；after actual marker bytes/SHA256；typed RejectOnce 与一次性 responder；terminal 与 cancel intent/deny/runtime exit 分开；完整含隐藏目录的 manifest、symlink/reparse fail closed；bounded Child cleanup，不声称 Windows Job-at-creation 或 tree containment。

## 原证据与评审历史

`history/pre-canonical-repair/` 复制了旧38文件，包括旧源码、review、review.sha256、target/hashes、7次全部证据，另有 ARCHIVE.json。旧 target=`85e8fae817a890d9f5e854ab062aa92cef400f9f0a0d9e7c2851d7eb18117a4f`，context=`6bf445105455338b4ed69e82c846db15bd87deabf8b402573bb105c96b9f0cc9`。

原 evidence 仍原地保存，逐文件 SHA256 对照见 `evidence/canonical-repair/prior-evidence-preservation.json`，全部 unchanged。旧7次加本轮1次，共8次历史记录；本轮process-evidence仅包含当前1次，不混淆数量。旧review PASS仅对旧target有效，新代码等待独立review。

## 验证命令与结果

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`，TASK=`.trellis/tasks/09-26-cb5-004-cancel-permission-contract`。

| 命令 | 结果 |
|---|---|
| `$env:CARGO_HOME=(Resolve-Path TASK/.cargo-cache).Path; cargo --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' test --offline --manifest-path TASK/harness/Cargo.toml` | PASS exit0，11 unit +7 integration=18/18，0 ignored，最终Rust实现 |
| `python TASK/test_run_probe.py` | PASS exit0，4/4，最终Python实现 |
| `python TASK/run_probe.py diagnostic` | C FAIL exit1；唯一真实修复probe；harness exit0非合同PASS |
| `python TASK/run_probe.py --summarize` | PARTIAL exit1，只汇总；三个场景 NOT_RUN: C_GATE_FAILED，无CLI调用 |
| `cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check` | PASS exit0 |
| `git diff --check` / 父会话 cached check | PASS exit0 |
| 父会话 scope-verification | HEAD 314687f9ec0ab8bb6115971cf1edc6e8ef116b2d不变；755 tracked SHA256 delta0、产品delta0 |

四类新增 Rust 合同测试全部通过：真实 argv 仅canonical；notification穿插的 session/new响应先于 mode request、mode ACK先于prompt；auto来自真实 typed目录，缺失就停；failed/unknown new不发送mode/config/prompt。保留13个既有测试覆盖cancel/permission identity、malformed schema、duplicate/late、terminal/timeout、late updates、cleanup故障、marker实际hash、manifest和脱敏。Python覆盖实际hash、C gate拒绝后续、已有durable identity禁止重放、本次结果缺失失败。

无生产Cargo/src/src-tauri/主文档/根.gitignore改动；旧 task 和 .zed 保留，没有 Git commit、没有新task、没有CB5-005。仅task-local缓存/target排除review正文，全部自编源码与fake/tests纳入新freeze。按用户要求停止，等待Host Gate。

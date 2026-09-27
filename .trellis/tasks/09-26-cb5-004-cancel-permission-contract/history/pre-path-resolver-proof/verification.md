# CB5-004 Gold Band JSONL 诊断 verification

结果：**LAUNCHER_UNAVAILABLE_OR_NO_ACP_HANDSHAKE**。唯一 npx 尝试在 initialize 写入后 EOF，进程 exit1；没有收到 initialize 响应，也没有发送 session/new。不能认定 npm/network 具体原因，不能据此证明 backend 或认证问题。停止等待 Host Gate。

## Help / auth status

canonical `node.exe + codebuddy --version` 返回2.158.0，exit0；`--help` exit0。帮助明确列出 `--serve`、`--host`、`--port`、`--auth`，但没有只读 health/auth status endpoint。doctor 只针对auto-updater，不是会话认证状态。

依用户要求，serve/auth检查记 **NOT_AVAILABLE**，healthy/authenticated=null；没有启动serve、没有猜endpoint或读取用户配置/凭据。见 `evidence/gold-band-wire-repro/serve-auth-status.json`。

## 唯一真实尝试

逻辑launcher：`C:\Users\lifei\AppData\Roaming\npm\npx.cmd -y @tencent-ai/codebuddy-code@2.158.0 --acp`。Windows通过`C:\Windows\System32\cmd.exe /d /s /c`执行这一个batch launcher，未更换launcher或重试。完整nonsecret argv在attempt-started.json和wire-result.json。

initialize参数完全按用户指定：protocolVersion=1；clientCapabilities.elicitation.form={}；clientCapabilities._meta含subagent-transcript=true、parameterizedModelPicker=true；clientInfo.name=serena-desktop-gold-band-repro、title=SerenaDesktop Gold Band Repro、version=0.1。wire中只有这一条request(id=1)，随后EOF。

- initialize matched response：无；agentCapability keys 未知。
- session/new：NOT_SENT；因此没有new params实际wire、sessionId或HTTP500新证据。
- interleaved notification=0，pre-route buffer=0/64；wrongid response=0。
- close：NOT_RUN；prompt/model：从未允许或发送。
- owned wrapper/launcher exitCode=1；directChildReaped=true，readersJoined=true；taskkill exit1原样记录，不能视为树清理成功证明。
- stderr仅drain计2399字节，不保存或展示内容；环境、headers、credentials没有落盘。
- fresh临时工作区 before={}、after={}、delta=[]，workspaceDeleted=true。
- 不声称Windows Job-at-creation或进程树containment。无后续真实调用。

一手证据：`wire-result.json`、`wire.jsonl`、`attempt-started.json`。一次性durable sentinel阻止unknown outcome后重放。实现用极小JSONL client直接收发，不用高层SDK helper。

## 判定边界

| 用户判定分支 | 本轮支持情况 |
|---|---|
| wire PASS，说明material wire/launcher组合与此前不同 | 未达到，未完成initialize |
| wire HTTP500 + auth healthy，更支持backend/service | 未达到，本轮没有new请求且auth未知 |
| auth unhealthy，指向认证/session backend前置问题 | 未达到，没有auth status证据 |

因此只能报告**证据不足 / launcher unavailable或未形成ACP握手**。已尝试的material差异是固定版本npx launcher及明确初始化capabilities/clientInfo；不能把它们中的某一字段当作已证实原因。产品协议不变。

## 测试与检查

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`；TASK=`.trellis/tasks/09-26-cb5-004-cancel-permission-contract`。

| 命令 | 结果 |
|---|---|
| `python TASK/test_gold_band_probe.py` | PASS exit0，最终10/10；真实尝试前原9/9通过 |
| `python TASK/gold_band_probe.py` | exit1；唯一真实launcher尝试，initialize后EOF |
| `cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check` | PASS exit0 |
| `git diff --check` / 父会话cached check | PASS exit0 |
| 既有Rust18tests / runner4tests | 本轮NOT_RERUN；源码未改，沿用上一轮18/18、4/4证据，明确不称新执行 |
| HEAD / tracked baseline | 父scope：HEAD314687f9ec0ab8bb6115971cf1edc6e8ef116b2d不变，755tracked hashes delta0 |

10tests覆盖exact initialize/launcher、notification interleave、wrongid忽略、EOF、malformed JSON/response、new失败阻断close/prompt、非空sessionId gate、64条preroute有界、禁止prompt方法。close只依据实际initialize advertised sessionCapabilities.close，成功new后才能发送。

真实运行后按独立review P1修复，所有通知wire样本共用64条上限、wrong-id response样本16条上限，aggregate计数独立保留；新增100组混合notification/wrong-id洪流测试，确保最终正确响应仍被匹配；实际唯一运行通知数为0，不改变该事实。preflight记录真实执行时source hash，verification-results记录最终source hash；无真实重跑。

## 历史保留与范围

原8失败（旧7 + canonical repair1）保持原文件逐字节不变：28个旧evidence文件全部hash匹配，证明见本轮prior-evidence-preservation.json。`history/pre-gold-band-wire-repro/`保存旧48文件及ARCHIVE.json，包括旧review/target/hash；更早history保持不变。

本轮新增source仅gold_band_probe.py、test_gold_band_probe.py；新证据只写evidence/gold-band-wire-repro/。根verification/review-context/新freeze为报告更新，旧版已归档。产品src/src-tauri/主Cargo、根.gitignore、用户settings、其它task、.zed未改；没有commit、新task、真实cancel/permission或CB5-005。

当前累计：旧8次受控ACP失败 + 本轮1次launcher尝试；本轮session/new次数0。以前C Gate失败结论保留，原三场景PARTIAL/NOT_RUN不被本轮覆盖。等待独立review和Host Gate。

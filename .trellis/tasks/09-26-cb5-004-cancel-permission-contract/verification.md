# CB5-004 Host 真实合同 PASS（当前有效状态）

Fresh Session prerequisite、cancel-before、cancel-after、permission-deny 均有 Host 真实 wire 证据，**CB5-004 contract-test PASS**。CB5-003 Fresh Execute PASS 保持；CB5-005 未开始，其 CB5-004 依赖已满足，是否进入后续任务由 Host 决定。

- before：exact prompt sequence 6，correlated activity 且 manifest 为空后 cancel 29，exact prompt terminal 30 / `cancelled`，最终 delta=[]。
- after：真实 catalog 广告 auto，typed set_mode request 6 / ACK 10（通知穿插），prompt 11；磁盘 marker 27 bytes、SHA256 `613841732b16579f9417ffa2ffeb2a86dab63957b0c8381976403c6ee4960301` 后 cancel 43，terminal 47 / `cancelled`。marker 保留且为唯一 delta；cancel 不回滚。
- permission：default Always Ask；真实 request 91 / RPC id 0，exact session/tool identity，typed RejectOnce 选择广告 optionId `reject`，response 92；无 session/cancel，exact prompt terminal 96 / `cancelled`，deny 前后 manifest 均为空。deny 本身不是 terminal，本次 terminal 由 Provider 独立返回。
- 三个 fresh temp cwd 独立，direct child cleanup/reap、streams close 与 workspace delete 均成功。进程 cleanup exitCode=1 是 terminal 之后 owned-child termination 记录，不替代 ACP terminal，也不证明 Windows Job-at-creation/tree containment。

[验收](evidence/host-cancel-permission-proof/acceptance.json)、[合并 cancellation](evidence/host-cancel-permission-proof/cancellation.jsonl)、[permission wire](evidence/host-cancel-permission-proof/permission.jsonl)、[进程证据](evidence/host-cancel-permission-proof/process-evidence.json) 已逐项核对 exact identity/sequence/manifest/hash，而非仅采用 runner PASS。permission options 是 CodeBuddy 2.158.0 本次观察，不能跨版本写死白名单；生产始终按 typed kind + advertised ID 决策。`canCancel` 还需 CB8-001 implementation PASS 才可 advertise。Cancel/deny 不授权 Claim release；Phase 6/7 Runtime/Claim 冻结边界不变。

本次收口仅核对现有 evidence 与文档；真实调用 0，harness/产品源码无修改，无commit。旧 raw evidence/hash/ready review-target 保留。独立 review 与最终 Host Gate 由父会话收口；不进入 CB5-005。

---

# 以下为原始历史报告（superseded，原文保留）

# CB5-004 Host 补充验证：Fresh Session 前置已解除

当前有效状态：**CodeBuddy ACP `initialize → session/new` 已在 Host 受管命令环境真实 PASS；CB5-004 的 cancel / permission 合同仍待验证。**

Host 在 2026-09-27 使用 Gold Band 0.17.2 的实际启动事实重新复验：

- 本机 Gold Band 周期 Doctor 对 `codebuddy-code` 当前持续 `session/new=ok`；实际子进程为 `C:\\nvm4w\\nodejs\\npx.cmd -y @tencent-ai/codebuddy-code@2.158.0 --acp`。
- `evidence/host-exact-launcher-proof/`：按 Gold Band 的 launcher、标准用户目录环境、普通 Win32 cwd 和 Gold Band initialize shape 执行，`initialize` 返回 ACP v1，`session/new {cwd,mcpServers:[]}` 返回非空 sessionId；期间允许 `session/update` 插入并按 JSON-RPC id 正确关联 response。未发送 prompt，workspace delta=0。
- `evidence/host-direct-codebuddy-proof/`：保留 Serena 首版“不自动安装”的方向，直接使用 `C:\\nvm4w\\nodejs\\node.exe + 已安装 CodeBuddy CLI 脚本 + --acp`，同样真实完成 `initialize → session/new`，返回非空 sessionId；未发送 prompt，workspace delta=0。
- 因此此前 nested Agent probe 中的 HTTP500 / EOF / NPM_EPERM **不能再作为 CodeBuddy ACP Fresh Session 不兼容证据**；它们属于不同 runner/launcher 环境下的诊断历史。下面原始报告保留用于审计。
- 仍未据此提升 `canCancel` 或 permission deny 能力；CB5-004 的剩余 Gate 只针对 cancel / permission 合同。

产品实现前置已收敛为：Provider Runtime 需要独立于通用 CommandRun 的 Windows 启动环境；内部 canonical Workspace Authority 保持不变，对外部进程 cwd 做 Win32 路径投影；权限 mode 在 Session 建立后根据 Provider 实际返回目录通过 ACP 设置，不在 CLI 启动参数中预注入。

直接证据：[Host exact launcher wire](evidence/host-exact-launcher-proof/wire-result.json)、[Host direct CodeBuddy wire](evidence/host-direct-codebuddy-proof/wire-result.json)。前者 sequence 4 是 session/update(config_option_update)，sequence 5 才是 id=2 的 session/new response，证明 response 必须按 JSON-RPC id 匹配。两者 promptSent=false、delta=[]，windowsJobAtCreationProven=false、treeContainmentProven=false；不能据此声称 Cancel / Permission 或生产 Runtime safety PASS。

文档同步：技术方案 §14.1、§14.4、§15.1、§28.0 与任务分解 Phase 5/6/7 已按此有效状态修正。CB5-003 Fresh Execute PASS 保持；本卡剩余 only Cancel / Permission。以下 exact launcher NPM_EPERM 报告整体为 **superseded diagnostic history**，“本轮/没有真实 new PASS/等待 Gate”等措辞仅描述当时 nested runner，不覆盖本页顶部 Host PASS；原始 evidence 不变。本次只改文档，停止等待 Host Gate。

---

# 历史报告 — CB5-004 exact CMD launcher proof verification（superseded diagnostic history）

结果：**LAUNCHER_UNAVAILABLE_OR_NO_ACP_HANDSHAKE；本轮安全 stderr 分类 NPM_EPERM**。唯一精确launcher尝试在initialize发送后EOF，没有initialize响应、没有session/new。不重试、不改npm缓存或权限，停止等待Host Gate。

## 精确launcher / cwd

实际交给 `subprocess.Popen` 的第一个参数为原始Windows字符串：

```text
"C:\Windows\System32\cmd.exe" /e:ON /v:OFF /d /c ""C:\nvm4w\nodejs\npx.cmd" -y @tencent-ai/codebuddy-code@2.158.0 --acp"
```

没有 `/s`，没有 Roaming wrapper，没有 list2cmdline 或 `shell=True` 再次改写。4个新增测试直接mock实际Popen边界核验参数类型和值；父会话独立prelaunch核对PASS后才启动唯一真实尝试。

cwd=`C:\Users\lifei\AppData\Local\Temp\cb5-004-gold-band-lvmkmpvt`，普通Win32 drive路径，无`\\?\`。harness保留canonicalWorkspaceRoot，只有external cwd边界将local verbatim drive形态投影普通drive路径；UNC/relative拒绝，不改变Authority定义。

child PATH至少NVM目录、System32、Windows，随后复用前轮explicit→process→HKCU→HKLM→common组合、registry目录变量展开和Windows去重。不存完整PATH/env，不全局改环境。resolved node/npx路径见launcher-resolution.json。

## 实际wire与清理

- 用户指定Gold Band initialize完整shape原样发送，RPC id=1。
- stdout随后EOF，没有任何matched initialize response、capabilities、session/new、close或prompt。
- `stderrClassification.firstSafeCategory=NPM_EPERM`；最多8192 bytes内存窗口，只输出静态enum，rawSaved=false。累计drain2462字节，无错误自由文本/credentials落盘。
- wrapper/launcher exitCode=1；directChildReaped=true、readersJoined=true；taskkill exit1如实保留，不视为树containment证明。
- workspace before={}、after={}、delta=[]，workspaceDeleted=true。
- 此轮尝试1，session/new真实发送0；没有重试、额外version运行、替代launcher或其它真实场景。

证据在 `evidence/exact-launcher-proof/` 的attempt-started.json、launcher-resolution.json、wire-result.json、wire.jsonl、conclusion.json。preflight包含独立prelaunch PASS及source hash，sourceAtRun与最终source相同。

## 结论边界

精确flags/quotes、普通cwd和child PATH前置已满足，本轮收到npm静态EPERM错误分类，因此只能记录本轮launcher前置失败；尚未进入可判断ACP session/new的阶段。不能据此推定特定文件、缓存、网络或权限根因；不能回填此前EOF或HTTP500的原因；不能声称ACP不兼容。

没有真实new PASS，因此不提升cancel/permission Acceptance，不将Host已发现的launcher/environment边界假设包装为本轮已闭合session/new。生产源码/协议/Runtime保持不变，没有为EPERM追加任何权限或cache变更。

## 检查

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`，TASK=`.trellis/tasks/09-26-cb5-004-cancel-permission-contract`，PY=`C:/Users/lifei/AppData/Local/Programs/Python/Python312/python.exe`。

| 命令/检查 | 结果 |
|---|---|
| `& PY TASK/test_exact_launcher_probe.py` | PASS exit0，4/4；actual Popen str/flags/quotes、cwd投影、stderr静态分类 |
| `& PY TASK/test_path_resolver_probe.py` | PASS exit0，6/6 |
| `& PY TASK/test_gold_band_probe.py` | PASS exit0，10/10 |
| 独立prelaunch只读核对 | PASS，核对实际raw commandline/cwd/env接线后放行唯一调用 |
| `& PY TASK/exact_launcher_probe.py` | exit1；唯一真实initialize后EOF，NPM_EPERM |
| `& C:/Users/lifei/.cargo/bin/cargo.exe fmt --manifest-path TASK/harness/Cargo.toml -- --check` | PASS exit0 |
| `git diff --check` / 父cached check | PASS exit0 |
| 既有Rust18tests / oldrunner4tests | NOT_RERUN；源码未改，保留前轮18/18、4/4证据 |
| 父scope-verification | HEAD314687f9ec0ab8bb6115971cf1edc6e8ef116b2d、755tracked hashes delta0 |

20个相关Python tests均在真实调用前通过，之后没有source变化。未以fake测试替代真实合同证据。

## 保留与范围

`history/pre-exact-launcher-proof/`保存旧71文件加ARCHIVE，包括旧源码/review/target/hash。原47个evidence文件逐字节hash不变，包含旧8次ACP失败及前两轮npx尝试；本轮另加1个exact launcher尝试，累计8+2+1，当前new请求0。

prior target=`eeb55da53a129141f8736bf28f8f50c3d76e99fbac4898466f7e21ba6e7dab8d`。新增task-local exact launcher和tests、最小共享接线、当前证据/报告；产品src/src-tauri/主Cargo、用户settings、根.gitignore、其它task与.zed不变。没有Git commit、新task、真实cancel/permission或CB5-005。等待独立review和Host Gate。

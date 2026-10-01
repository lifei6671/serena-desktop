# CB5-004 child PATH resolver proof verification

结果：**resolver前置 READY，但真实wire仍 LAUNCHER_UNAVAILABLE_OR_NO_ACP_HANDSHAKE**。唯一指定NVM npx尝试在initialize发送后EOF，exit1；没有握手响应，没有session/new请求。不能认定CHILD_ENV_PATH_RESOLUTION已被本轮证明为根因，也不能把之前canonical node的HTTP500归因于PATH。停止等待Host Gate。

## 实施与范围

新增task-local `path_resolver_probe.py`、`test_path_resolver_probe.py`。复用既有最小JSONL client，仅增加可传child env和nonsecret launcher diagnostics，不改变exact initialize或请求流程。

PATH源顺序：explicit agent PATH（本次用户指定的NVM目录）→current process PATH→HKCU Environment Path→HKLM Session Manager Environment Path→platform common dirs。registry仅精确读取Path及NVM_HOME/NVM_SYMLINK等目录变量白名单，支持%VAR%展开、Windows大小写不敏感去重；未枚举其它registry环境。未知变量/相对目录不进入执行搜索。

bare executable仅选择.exe/.com/.cmd/.bat，不选择.ps1或无扩展shim；bare npx解析到npx.cmd。子进程其余环境按继承机制原样传递，只覆写PATH，不解析或记录其它值。没有全局环境修改，没有完整PATH/env/token写证据。

## 唯一实测

- launcher exact：`C:\nvm4w\nodejs\npx.cmd -y @tencent-ai/codebuddy-code@2.158.0 --acp`，Windows cmd /d /s /c仅执行该batch。
- resolvedNpxPath=`C:\nvm4w\nodejs\npx.cmd`。
- resolvedNodePath=`C:\nvm4w\nodejs\node.exe`，siblingNodeExists=true。
- childPathContainsNvmSymlink=true；HKCU/HKLM Path sources均读取成功；resolvedBareNpxPath同指定路径。
- resolvedCodebuddyPath=null仅表示裸命令搜索未找到；实际启动器仍按用户要求使用npx固定包，不fallback。
- initialize wire完全复用用户指定protocolVersion1、elicitation.form、两项_meta flags及clientInfo。仅这一条id=1请求写入后EOF。
- initialize response：无；session/new/close/prompt：NOT_SENT；notification=0。
- launcher exitCode=1，stderr只drain计2462字节，未保存原文；不推断npm/network具体错误。
- directChildReaped=true、readersJoined=true；taskkill exit1保留，不声称树清理成功或Windows Job-at-creation。
- fresh工作区 before={}、after={}、delta=[]、workspaceDeleted=true。
- 本轮launcher尝试1，session/new实际发送0；无retry、无更换launcher。

证据均在 `evidence/path-resolver-proof/`：launcher-resolution.json、attempt-started.json、wire-result.json、wire.jsonl、conclusion.json。sourceAtRun与最终source完全相同。

## 结论限制

本轮证明指定npx/node路径和child PATH前置已满足，但仍未得到ACP握手。未达到用户要求的wire PASS，因此不输出已证实的CHILD_ENV_PATH_RESOLUTION根因或产品修复结论。此前Roaming wrapper无Node的Host假设与本轮已满足前置的事实分别保留，不否认、不扩大为对所有历史失败的因果判断。

没有新增真实cancel/permission，不进入任何后续场景；原HTTP500/PARTIAL、canonical C失败、Gold Band EOF结论原样保留。

## 检查

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`，TASK=`.trellis/tasks/09-26-cb5-004-cancel-permission-contract`，PY=`C:/Users/lifei/AppData/Local/Programs/Python/Python312/python.exe`。

| 命令/检查 | 结果 |
|---|---|
| `& PY TASK/test_path_resolver_probe.py` | PASS exit0，6/6 |
| `& PY TASK/test_gold_band_probe.py` | PASS exit0，10/10 |
| `& PY TASK/path_resolver_probe.py` | exit1；唯一实测initialize后EOF，未new |
| `& C:/Users/lifei/.cargo/bin/cargo.exe fmt --manifest-path TASK/harness/Cargo.toml -- --check` | PASS exit0 |
| `git diff --check` / 父cached check | PASS exit0 |
| 既有Rust18tests / runner4tests | 本轮NOT_RERUN；源码未改，保留上一轮18/18、4/4事实 |
| 父scope-verification | HEAD314687f9ec0ab8bb6115971cf1edc6e8ef116b2d不变，755tracked hashes delta0 |

6个resolver测试验证：Roaming npx缺node在launcher层失败且acpFailure=false；指定NVM node/npx候选；五层PATH源优先序、registry变量展开及去重；只选.cmd不选.ps1/shim；launcher缺依赖完全不调用JSONL；未知变量/相对目录拒绝。10个既有JSONL tests验证exact初始化、RPCid/通知穿插、EOF/malformed、新会话身份gate、禁止prompt及混合洪流有界。

## 历史与交付

`history/pre-path-resolver-proof/`保存旧60文件加ARCHIVE.json（含旧源码、review、target/hash）。原38 evidence文件逐字节hash不变，包含所有旧8次ACP失败和前一轮Gold Band launcher尝试。当前累计旧8次 + Gold Band1次 + path proof1次；本轮new发送0。

prior target=`fbdb27c954169167021f10fe5a3017b52173c5e532887495b783379758d8fa4f`；新freeze绑定当前实现与本轮证据。产品src/src-tauri/主Cargo、用户settings、根.gitignore、其它task与.zed保持不变，没有commit、新task或CB5-005。等待独立review和Host Gate。

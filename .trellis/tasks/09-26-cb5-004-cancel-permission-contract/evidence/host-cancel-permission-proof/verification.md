# CB5-004 Host 真实合同 PASS（当前有效状态）

Fresh Session prerequisite、cancel-before、cancel-after、permission-deny 均有 Host 真实 wire 证据，**CB5-004 contract-test PASS**。CB5-003 Fresh Execute PASS 保持；CB5-005 未开始，其 CB5-004 依赖已满足，是否进入后续任务由 Host 决定。

- before：exact prompt sequence 6，correlated activity 且 manifest 为空后 cancel 29，exact prompt terminal 30 / `cancelled`，最终 delta=[]。
- after：真实 catalog 广告 auto，typed set_mode request 6 / ACK 10（通知穿插），prompt 11；磁盘 marker 27 bytes、SHA256 `613841732b16579f9417ffa2ffeb2a86dab63957b0c8381976403c6ee4960301` 后 cancel 43，terminal 47 / `cancelled`。marker 保留且为唯一 delta；cancel 不回滚。
- permission：default Always Ask；真实 request 91 / RPC id 0，exact session/tool identity，typed RejectOnce 选择广告 optionId `reject`，response 92；无 session/cancel，exact prompt terminal 96 / `cancelled`，deny 前后 manifest 均为空。deny 本身不是 terminal，本次 terminal 由 Provider 独立返回。
- 三个 fresh temp cwd 独立，direct child cleanup/reap、streams close 与 workspace delete 均成功。进程 cleanup exitCode=1 是 terminal 之后 owned-child termination 记录，不替代 ACP terminal，也不证明 Windows Job-at-creation/tree containment。

[验收](acceptance.json)、[合并 cancellation](cancellation.jsonl)、[permission wire](permission.jsonl)、[进程证据](process-evidence.json) 已逐项核对 exact identity/sequence/manifest/hash，而非仅采用 runner PASS。permission options 是 CodeBuddy 2.158.0 本次观察，不能跨版本写死白名单；生产始终按 typed kind + advertised ID 决策。`canCancel` 还需 CB8-001 implementation PASS 才可 advertise。Cancel/deny 不授权 Claim release；Phase 6/7 Runtime/Claim 冻结边界不变。

本次收口仅核对现有 evidence 与文档；真实调用 0，harness/产品源码无修改，无commit。旧 raw evidence/hash/ready review-target 保留。独立 review 与最终 Host Gate 由父会话收口；不进入 CB5-005。

---

## 历史 harness-ready 报告（superseded，命令不得再次执行）

# Harness-only verification

结论：HARNESS_READY，deterministic verified。真实 before / after / permission：NOT_RUN by implement agent；等待 Host 单次执行。未调用 CodeBuddy version/new/diagnostic。Host 已验证的 Fresh prerequisite PASS 持续有效；旧 nested 错误不是兼容性 gate。

- Rust official SDK 2.2.0：新 binary 12 unit + 9 fake integration = 21 PASS，0 failed，0 ignored。参见 rust-tests.txt。
- Python Host runner：3 PASS，参见 python-tests.txt。
- cargo fmt --check：exit 0。
- 精确 init：SDK UntypedMessage 发出实际指定 Gold Band shape；fake peer 校验收到 wire 的整体参数。SDK typed InitializeResponse 校验 protocolVersion；new/mode/prompt/cancel/permission 保持 official typed。
- review 修复：init evidence 从实际 wire 做白名单投影；回归测试通过 safe_rows 实际路径验证异常 protocolVersion 不被期望值替代。正式 runner sentinel create-new + fsync；real binary 固定 proof 输出路径且 identity create-new，不能改输出路径重放。
- before activity 与零 delta、after actual marker/hash、typed denial identity/duplicate/malformed/no-deny、terminal/timeout、late updates、cleanup failures、new失败阻断mode/prompt、early config notification及RPC ID等均由unit/fake覆盖。无fake证据冒充真实合同。
- 全量 hidden manifest + symlink/reparse fail closed；落盘文件内容只有size/hash。wire内存有4MiB上限（接收最多超出一个8192字节片段）并以错误收敛，不无限保存。
- 子进程清理有界，仅声明 owned direct child；不证明 Job-at-creation/tree containment。
- scope-verification.json：相对本轮 Host baseline 755 tracked 文件无新增变化，保留两份 Host 文档修改；89个旧报告/证据hash保持不变；产品tracked delta 0；git diff --check/cached check PASS。HEAD不变。无commit，未进CB5-005。

命令（普通 Windows cwd，CARGO_HOME仅当前shell）：

```powershell
$env:CARGO_HOME=(Resolve-Path .trellis/tasks/09-26-cb5-004-cancel-permission-contract/.cargo-cache).Path
& C:/Users/lifei/.cargo/bin/cargo.exe --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' test --offline --manifest-path .trellis/tasks/09-26-cb5-004-cancel-permission-contract/harness/Cargo.toml --bin host_cancel_permission --test host_contracts
& C:/Users/lifei/AppData/Local/Programs/Python/Python312/python.exe -m unittest discover -s .trellis/tasks/09-26-cb5-004-cancel-permission-contract -p test_host_cancel_permission.py
& C:/Users/lifei/.cargo/bin/cargo.exe fmt --manifest-path .trellis/tasks/09-26-cb5-004-cancel-permission-contract/harness/Cargo.toml -- --check
```

所有测试为 Windows 本机证据，不宣称 Linux 验证。本次无旧suite复跑（旧source未修改），新suite包含相关合同测试。真实验收与最终Host Gate尚待Host执行；README列出三条唯一入口命令。

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

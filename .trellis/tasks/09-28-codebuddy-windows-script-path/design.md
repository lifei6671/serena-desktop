# 设计

## 权威与边界

`discovery.rs` 继续通过 `fs::canonicalize` 冻结 npm wrapper 解析出的 `node.exe` 与主脚本 identity。修复放在 `windows_launcher.rs` 的 `LaunchRequest::from_resolved`，因为这里是 canonical resolved spec 转换为 `CreateProcessW` executable/argv 的最后外部进程边界。

## 最小实现

1. 保留现有 Workspace `ExternalProcessPath` 及 cwd/UNC 语义不变。
2. 在同一 launcher 模块增加窄的本地文件路径投影与验证函数：
   - 输入必须是绝对路径；
   - `\\?\X:\...` 去除 verbatim prefix；ordinary `X:\...` 保持不变；
   - ordinary/verbatim UNC 和其他 verbatim namespace 明确拒绝；
   - 输入与投影分别 `fs::canonicalize`，要求均为 regular file，并用现有 Windows UTF-16 ordinal identity 比较确认 canonical 结果相同。
3. 仅对已通过 `validate_resolved_launch_spec` 的 npm Node 主脚本 argv 应用该投影；direct executable 的 frozen `--acp` argv 不变。
4. `ResolvedLaunchSpec` 仍保存 canonical identity；`LaunchRequest` 保存外部进程可消费的 projected argv。无 shell、无 PATH 重解析。

## 测试设计

- 临时 regular file：canonical verbatim 输入投影为 ordinary local-drive path。
- ordinary file：输出不变。
- 两个不同临时文件：identity revalidation 拒绝 mismatch；missing/directory 拒绝。
- relative、非法 verbatim namespace、ordinary/verbatim UNC 拒绝。
- `LaunchRequest::from_resolved`：确认 Node script argv 已投影，resolved spec 本身未被修改。
- 本机存在 `node.exe` 时，用临时 JS fixture 通过真实 Windows launcher 输出 `process.argv[1]`，断言无 `\\?\`；无 Node 时只报告该 fixture unavailable，不把它冒充 PASS。

## 风险控制

- 不改变 discovery provenance、Catalog、Provider/Work/Agent 状态或 ACP 消息。
- 文件投影只进入私有 launcher request，不反向写回 canonical authority。
- 不泛化为共享跨平台抽象，避免扩大 blast radius。

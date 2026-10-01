# CB4-002 交付证据

仅完成本地 Role Routing Editor：Agent 接入后、Composer 前显示五个角色，复用 shadcn Select 与 `agent_provider_set_role_route`。支持 null 清空、disabled 可选与保留、unknown ID 可见和主动改绑。每个角色独立 pending、提交与失败回滚；mutation 开始/结束代次防止旧轮询回写，后续新轮询可继续接管。未修改后端语义、Remote、Runtime 或 Execution frozen identity，未进入 CB4-003，未提交 Git。

本次代码增量仅 `src/AgentPanel.tsx`、`src/api.ts`、`src/styles.css`、`src/AgentPanel.test.mjs`。前序修改保留；task 保持 `in_progress` 等待 Host Gate。

## 验证

所有命令 cwd：`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。Rust 为原生 Windows，前端为 Node/JSDOM；不代表 Linux 或原生 Tauri 视觉验收，未运行 WSL。

| 命令 | 结果 |
| --- | --- |
| `node --test src/AgentPanel.test.mjs src/App.test.mjs` | PASS，101/101 |
| `npm test` | PASS，156/156 |
| `npm run build` | PASS，tsc + Vite |
| `npm run lint` | PASS |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml provider_policy -- --test-threads=1` | PASS，14/14 |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib mcp::registry::tests:: -- --nocapture` | PASS，20/20 |
| `cargo check --manifest-path src-tauri/Cargo.toml --locked` | PASS |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | PASS |
| `git diff --check` | PASS |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings` | FAIL，exit 101，仅既有 `usage_tests.rs:987 await_holding_lock`，按要求未修 |

成功命令均 exit 0。首次测试夹具误拦 JSDOM RAF 定时器，引起焦点断言失败与 DOM 诊断 OOM；已修复定时器隔离，并保留等价焦点身份断言，最终 focused/full 全部通过。具体过程见角色矩阵，不将中断或失败计为 PASS。

## 证据

- [角色矩阵及 set/clear/failure/stale-poll 示例](evidence/frontend-role-matrix.md)
- [精确命令与结果](evidence/validation.json)
- [focused 结果与哈希](evidence/frontend-final.json)
- [本次四文件增量](evidence/implementation.diff)
- [四文件前后 SHA256](evidence/changed-files.json)
- [基线保留核对](evidence/baseline-preservation.json)
- [独立审查](evidence/review.md)

两份权威文档 SHA256 与 Host 提供值一致。Remote registry/hash 20 项回归通过，registry 与 Remote 生产代码相对启动时字节未变。

933 个基线文件无丢失。收尾发现 `src-tauri/src/mcp/start_routing_tests.rs` 发生会话外并发变化：主会话、实施与审查 Agent 均未编辑该文件，保留并排除于本次交付，不归因于本任务。Rust 验证对应执行时快照，不声称已验证此后出现的外部测试修改；它不改变本次生产 Authority 或 Remote descriptor。

冻结清单 SHA256：`121371b97b3a34bd999135b99296945478be51bc87c0f7aaad8e82e189c441a0`。最终审查结果见 `evidence/review.md`，不替代 Host Gate。

本次策略已记录于 task design、实现注释与证据，没有需要扩展通用 spec 的新架构约定。项目内及全局未找到可加载的 trellis-start skill，已读取现有 `.trellis/workflow.md` 并启动 Host 预建任务。

独立交付审查：APPROVED，CHILD_AGENT / FULL_SCOPE，PASSED，Coverage COMPLETE，Freshness FRESH，无 P0/P1，审查修复轮次 0。

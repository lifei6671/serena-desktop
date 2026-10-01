# 独立 review context

Authority：用户最新指令只做 Stage A resume/load，真实 CLI 全 NOT_RUN，禁止生产变更/Git commit。PRD/design/README 是本次 delivery contract。code-delivery-review Strict（用户要求独立 review），Tier 3 protocol/process/persistence/privacy。

Ownership：本 task 的 Rust harness、Python fake、文档、metadata/证据均本 Agent 新增；transport.rs 复制自 CB5-004 后加入 frame/queue bounds/static cleanup errors。旧任务仅只读。baseline.json 原始基线只作 provenance，不是 review 实现对象。外部 .gitignore 改动保留。Trellis session pointer 为已授权启动。

Review target：review-target.json 的逐文件 SHA256 + aggregate。排除 .cargo-cache、Cargo target 中间文件，仅收录交付 binary hash；依赖通过锁文件固定。真实 evidence NOT_RUN ledger 是执行前状态，Host 后续 result 自成证据，不回写假 PASS。

Requirement map：typed/session/cwd/dispatch -> runtime.rs,evidence.rs + typed/wrong identity/failure tests；phase/privacy -> summarize + independent/ambiguous tests；sentinel/output -> main.rs,durable + replay/fixed args tests；workspace/reparse -> manifest + hidden/junction tests；cleanup/bounds -> transport/runtime + timeout/frame/kill-error tests；scenario integration -> main.rs + full_scenario test；Host handoff -> README/NOT_RUN。

只读审查整个冻结 scope，不执行真实 binary，不修改文件，不运行可能生成文件的检查。返回匹配 targetId、覆盖范围、证据支持的 P0/P1/P2、PASSED/BLOCKED/UNAVAILABLE。可以只读核验已记录测试证据与 source；禁止把 fake PASS 当 Host PASS。

Round 1 repair：前target ca4fd33b97909e76d35d6122df2c97312251a8451b5f3f14799351ee45cfedbd 的唯一P1已修复，变更只在 runtime identity validation、fake_peer invalid-new-id、regression test与验证记录。最终16项tests/build/fmt重新PASS。复审需核验新target全部hash并确认P1修复与交互范围。

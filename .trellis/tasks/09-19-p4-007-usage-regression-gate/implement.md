# P4-007 执行计划

1. 固定初始 HEAD、dirty inventory、Host 指定 production 文件 SHA256 和 package scripts；所有既有差异均视作用户工作。
2. 审计 P4-001～P4-006 evidence/test tree，把 G01～G17 映射到真实测试，先运行用户指定的最小 Gate；零测试过滤器必须改正而非通过。
3. 仅当测试失败明显属于 stale fixture 才做最小 test-only 修复；任何 production regression 停止修复并记录。
4. 运行 Product full 和 full lib，逐项核对冻结失败的完整名称与 signature。
5. 运行 package.json 指定的前端 test/lint/build，及 Cargo check/fmt 和 diff check。
6. 在 `research/verification.md` 写入 Gate 表、所有命令/exit code/totals、hashes、失败比较、变更与归因；不 commit/push。
7. 冻结 delivery target，进行完整只读自审（本任务仅证据文档，未引入 production 变更），确认新产物不覆盖用户既有工作。

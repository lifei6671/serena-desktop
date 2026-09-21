# P0-008 执行计划

1. 记录 Git baseline 和现有脏改动排除清单；创建本任务的 research 目录。
2. 用 discovery 等价候选链定位真实 vendor binary，固定 identity；版本不符即停止。
3. 编写最小 Python JSONL probe，先记录 initialize 的 capability/schema 线索，再收集 fresh、continue、terminal/late、checkpoint、restart/resume 帧。
4. 从 raw samples 编制时间线、字段表和 probe matrix；不把测试计划当成供应商事实。
5. 写入 verification contract，做 JSONL 可解析性、identity 重算和 scope review；不执行项目构建或修改生产文件。

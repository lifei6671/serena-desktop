# P0-008 证据设计

## 边界

本任务是固定 `codex-cli 0.153.4` App Server 的供应商 wire 合同，不实现 Serena Usage domain。唯一可执行产物是隔离的 Python stdio probe；它只启动真实 vendor binary，并将证据写入本 Task 的 `research/`。

## 身份与隔离

按 `src-tauri/src/agent/codex/discovery.rs` 的 PATH + `%APPDATA%\\npm` 候选顺序，仅接受 vendor 目录内的 `codex.exe`，拒绝顶层 npm shim。开始前以 `--version`、文件 hash、metadata 确认身份；非精确版本不启动任何 Usage probe。Probe cwd 与 `CODEX_HOME` 都是系统临时目录，prompt 为固定无敏感短文本。

## 证据模型

每个收发 JSON frame 写成 JSONL：`probeId`、全局 `seq`、单调与 UTC 时间、`direction`、`method`、`id`、完整 `payload`。仅将用户输入字段替换为等长占位文本；其他结构与数字不变。stderr 只记录必要诊断且走同一敏感键脱敏。

探针先以 initialize/initialized 和可调用的 schema/自描述能力建立可用方法集合，再运行：两次 fresh；一个同 Thread 第二 Turn；每个 successful Turn 在 `turn/completed` 后监听 4 秒；每个阶段调用候选同步读取方法并保留成功或 `-32601` 原始响应；最后在新进程 `thread/resume` 后尝试读取/最小 turn。

## 判定

只将真实 schema 或重复实测支持的字段、顺序与累计语义写为 observed/declared。没有稳定且明确证据时为 `UNPROVEN`。`complete` 仅在 terminal 后同步 checkpoint 或 pinned contract 明确说明最终 notification 覆盖 terminal 时成立；否则为 partial/unknown。participant set 只有在累计范围、字段存在性与 baseline 安全性三者均被证实时才可冻结。

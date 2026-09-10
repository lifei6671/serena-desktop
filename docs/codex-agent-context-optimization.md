# SerenaDesktop Context Optimization 技术方案 v0.3

## 1. 目标

在 SerenaDesktop MCP Broker 内增加一个实验性的 Context Optimization（上下文优化）能力。

第一版只验证：

> 大型 Source / Git Tool Result 被截断时，将 Broker 已捕获内容保存在本地，并允许 ChatGPT 按需继续读取，是否能够降低一次性上下文输入，同时保持代码阅读和 Review 的准确性。

核心模型：

```text
Source / Git
    │
    ▼
Captured Tool Text
    │
    ├── inline 部分 → ChatGPT
    │
    └── overflow → context-cache.db
                         │
                         ▼
                 context_retrieve
```

v0.3 不尝试解决所有上下文优化问题。

---

# 2. v0.3 范围

只支持：

```text
git_diff
git_show
source_read_file
context_retrieve
```

其中：

```text
git_diff
git_show
source_read_file
```

只做：

```text
Retrievable Overflow
```

即：

```text
保留精确文本
+
按字节分页
+
按需 Retrieve
```

不做：

```text
结构压缩

JSON Table Fold

去重

Path Grouping

Relevance Sampling

LLM Summary

Embedding

Semantic Search

Query Retrieve

CodeGraph Optimization
```

以下完全保持原状：

```text
source_list_dir
source_find_file
source_search_pattern
source_symbols_overview
source_find_symbol
source_find_references

git_status
git_log
git_branch
git_worktree_list

codegraph_explore

media

agent
```

特别是：

```text
Agent
Agent Runtime
agent-state.db
Workspace Claim
```

完全旁路 Context Optimization。

---

# 3. 当前实现事实

## 3.1 Source 当前超限是 Error

当前 `source_read_file`：

```text
结果 <= max_bytes
→ success

结果 > max_bytes
→ OUTPUT_LIMIT_EXCEEDED
```

Broker 不返回 Partial Content。

Serena 如果返回：

```text
The answer is too long
```

Adapter 同样转换为：

```text
OUTPUT_LIMIT_EXCEEDED
```

因此 Source 当前属于：

```text
fail-closed
```

---

## 3.2 Git 当前超限是 Truncated Success

当前：

```text
git_diff
git_show
```

超限后会：

```text
返回前缀

truncated=true
```

后续内容不可恢复。

因此 Git 当前属于：

```text
Truncate and Lose
```

---

## 3.3 两类工具不能使用相同失败回退

v0.3 保留这个历史差异：

```text
Git Cache Failure
→ 可以回退现有 truncated success

Source Cache Failure
→ 不能产生不可恢复 partial success
→ 回退 OUTPUT_LIMIT_EXCEEDED
```

这是有意设计。

---

# 4. Captured Tool Text

Context Store 保存的数据统一定义为：

> Backend Adapter 完成协议解析和 UTF-8 归一化以后，Broker 准备交付给公共 MCP Result 的 UTF-8 Tool Text。

不称为：

```text
Raw Bytes
Backend Raw Result
Git Raw stdout
Wire Payload
```

例如 Git 当前流程：

```text
Git stdout bytes
    ↓
process.rs
UTF-8 normalization
    ↓
Captured Tool Text
```

Serena：

```text
MCP Response
    ↓
Serena Adapter
text extraction
    ↓
Captured Tool Text
```

`context_retrieve` 只保证：

> 精确恢复 Broker 保存的 Captured Tool Text。

不保证恢复 Backend 未返回给 Broker 的数据。

---

# 5. `max_bytes` 公共语义

现有：

```text
max_bytes
```

语义保持不变：

> 调用方本次允许 Inline 返回的最大 UTF-8 字节数。

即：

```text
max_bytes
=
inline byte ceiling
```

启用 Context Optimization 后不得改变这个含义。

例如：

```text
source_read_file(max_bytes=131072)
```

不能因为 Optimizer 打开而只 Inline：

```text
32768 bytes
```

---

# 6. v0.3 固定预算

第一版直接冻结，不暴露高级配置。

| Tool               | 默认 Inline | Public `max_bytes` 上限 | Backend 字符限制 | Capture Byte Limit |
| ------------------ | --------: | --------------------: | -----------: | -----------------: |
| `source_read_file` |     32768 |                131072 | 131072 chars |             131072 |
| `git_diff`         |     65536 |                262144 |          N/A |             262144 |
| `git_show`         |     65536 |                262144 |          N/A |             262144 |

其中：

```text
Inline Limit
```

由：

```text
用户 max_bytes
或
默认 Inline
```

决定。

Capture Limit 不随调用方降低：

```text
git_diff(max_bytes=32768)

inline = 32768
capture = 262144
```

---

# 7. Source 的 char / byte 边界

Serena 使用：

```text
max_answer_chars
```

它是字符单位。

Broker 使用：

```text
max_bytes
Capture Limit
offset
returnedBytes
```

它们全部是 UTF-8 byte。

因此必须明确：

```text
Serena Backend Limit
≠
Broker Capture Byte Limit
```

v0.3 `source_read_file` 在 Context Optimization 开启时固定：

```text
max_answer_chars = 131072
```

这是：

```text
Backend Request Limit
```

而不是 Broker Byte Limit。

---

## 7.1 为什么不使用 `CaptureLimit / 4`

不采用：

```text
131072 / 4
=
32768 chars
```

这种保守换算。

否则对于大量 ASCII 的：

```text
Go
Rust
TypeScript
配置文件
```

Backend Capture 将退化到约 32K 字符，基本失去扩大 Capture 的价值。

---

## 7.2 Source 本地 Byte Clamp

Serena 成功返回文本后：

```text
UTF-8 bytes <= 131072
```

完整作为 Captured Tool Text。

如果：

```text
UTF-8 bytes > 131072
```

Broker：

```text
保留前 131072 bytes 以内的
最大合法 UTF-8 前缀

captureTruncated=true
```

因此中文、Emoji 等多字节内容不会因为：

```text
chars != bytes
```

直接导致整次调用失败。

---

## 7.3 Serena 自身拒绝返回

如果 Serena 自己返回：

```text
The answer is too long
```

或对应：

```text
OUTPUT_LIMIT_EXCEEDED
```

Broker 没有获得正文。

此时继续：

```text
OUTPUT_LIMIT_EXCEEDED
```

不得伪造 Captured Content。

---

# 8. Git Capture

v0.3 对：

```text
git_diff
git_show
```

把当前 `process::run()` 的 retained limit 从 Inline Budget 分离。

例如：

```text
用户没有 max_bytes
```

则：

```text
inline = 65536

capture = 262144
```

Git stdout 仍然按当前实现持续 drain 到 EOF，但 Broker 最多保留：

```text
262144 UTF-8 bytes
```

---

## 8.1 Capture 未达到上限

例如真实 Diff：

```text
180 KiB
```

则：

```text
capturedBytes = 180 KiB

captureTruncated = false

returnedBytes = 64 KiB
```

后续约：

```text
116 KiB
```

可以通过：

```text
context_retrieve
```

取得。

---

## 8.2 Capture 达到上限

例如实际 Diff：

```text
400 KiB
```

Broker 最多捕获：

```text
256 KiB
```

此时：

```text
captureTruncated=true
```

Store 最多只能恢复这：

```text
256 KiB
```

不得描述为：

```text
完整 Git Diff
```

如需更后面的内容：

```text
缩小 path
缩小 scope
重新查询
```

---

# 9. 何时创建 retrievalId

只有：

```text
capturedBytes > returnedBytes
```

时才创建：

```text
retrievalId
```

因为只有这种情况下 Store 中存在 Inline 尚未返回的数据。

如果：

```text
capturedBytes == returnedBytes
```

即使：

```text
captureTruncated=true
```

也不创建 retrievalId。

Retrieve 不会提供任何新增内容。

---

# 10. Source Partial Success 契约

Source 从 Error 转为 Partial Success 必须满足：

```text
1. Serena 成功返回正文

2. Broker 至少捕获了一段合法 UTF-8 Tool Text

3. capturedBytes > inlineBytes

4. Context Store 写入成功
```

满足后：

```text
success
+
truncated=true
+
retrievalId
```

---

## 10.1 Capture 本身不完整也允许 Retrieve

例如：

```text
Serena 返回 300 KiB UTF-8 text

Capture Limit = 128 KiB

Inline = 32 KiB
```

则：

```text
capturedBytes = 128 KiB
returnedBytes = 32 KiB

captureTruncated = true
```

仍然允许：

```text
retrievalId
```

因为 Store 至少可以恢复：

```text
32 KiB → 128 KiB
```

只是不能恢复 128 KiB 之后的内容。

---

## 10.2 没有可恢复 Overflow 时

例如：

```text
inline = 128 KiB
capture = 128 KiB

Backend Result > 128 KiB
```

此时：

```text
captureTruncated=true
capturedBytes == returnedBytes
```

没有额外 Captured Content 可以 Retrieve。

Source 不返回不可恢复 Partial Success。

保持：

```text
OUTPUT_LIMIT_EXCEEDED
```

---

# 11. `truncated` 与 `captureTruncated`

两个字段语义不同。

## truncated

公共：

```text
truncated=true
```

表示：

> Broker 已知当前 `text` 不能代表本次工具结果的完整可见文本。

包括：

```text
Inline Overflow

或

Broker Capture Truncation
```

对于 v0.3：

```text
truncated
=
inlineTruncated
OR
captureTruncated
```

---

## captureTruncated

只表示：

> Broker 是否因为本地 Capture Byte Limit 丢弃了已经收到或仍在产生的后续内容。

它不表示：

```text
Backend 在语义上绝对完整
```

例如 Serena 自己内部是否进一步裁剪结果，不由 Broker 推测。

---

# 12. Public Result

## 12.1 普通完整结果

不改变现有结果：

```json
{
  "workspace": {},
  "text": "...",
  "truncated": false
}
```

不增加：

```text
delivery
```

---

## 12.2 Retrievable Overflow

只有真正存在可 Retrieve Overflow 时增加：

```json
{
  "workspace": {},
  "text": "...",
  "truncated": true,
  "hint": "...",
  "delivery": {
    "mode": "retrievable_overflow",
    "capturedBytes": 183420,
    "returnedBytes": 65536,
    "captureTruncated": false,
    "retrievalId": "ctx_xxx",
    "expiresAt": 1789000000000
  }
}
```

其中：

```text
expiresAt
```

固定为：

```text
Unix milliseconds
```

---

## 12.3 Feature Off

如果：

```text
contextOptimization.enabled=false
```

Source / Git 返回必须保持现有格式：

```text
workspace
text
truncated
hint（如原逻辑需要）
```

不得出现：

```text
delivery
retrievalId
```

---

# 13. Hint

Hint 只说明事实和下一步。

不要求模型无条件读取全部结果。

---

## 13.1 Captured Content 完整

例如：

```text
当前只返回了结果的一部分，不要将当前文本视为完整结果。
如后续判断依赖未返回部分，请使用 context_retrieve 继续读取。
```

---

## 13.2 Capture 自身也已截断

例如：

```text
当前只返回了结果的一部分，且 Broker 捕获内容本身也已达到上限。
context_retrieve 可继续读取已捕获部分；如果任务需要更后面的内容，请缩小原查询范围。
```

禁止写成：

```text
生成 Patch 前必须读取完整 Diff
```

因为部分任务可能只依赖已经明确看到的局部内容。

核心要求只是：

> 不得把 Partial Result 错认为 Full Result。

---

# 14. Context Store

数据库：

```text
<AppData>/context-cache.db
```

不复用：

```text
agent-state.db
```

---

## 14.1 Schema

```sql
CREATE TABLE context_entries (
    id TEXT PRIMARY KEY NOT NULL,

    workspace_id TEXT NOT NULL,
    workspace_root TEXT NOT NULL,

    tool_name TEXT NOT NULL,
    args_hash TEXT NOT NULL,

    content_sha256 TEXT NOT NULL,
    capture_truncated INTEGER NOT NULL,

    content BLOB NOT NULL,
    content_bytes INTEGER NOT NULL,

    created_at INTEGER NOT NULL,
    last_accessed_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,

    UNIQUE(
        workspace_id,
        tool_name,
        args_hash,
        content_sha256,
        capture_truncated
    )
);

CREATE INDEX context_entries_expiry
ON context_entries(expires_at);

CREATE INDEX context_entries_lru
ON context_entries(last_accessed_at);
```

---

# 15. `args_hash`

`args_hash` 只计算：

```text
Semantic Args
```

即真正改变 Backend 查询内容的参数。

---

## git_diff

包括：

```text
scope
path
```

排除：

```text
max_bytes
```

---

## git_show

包括：

```text
reference
path
```

排除：

```text
max_bytes
```

---

## source_read_file

包括：

```text
relative_path
start_line
end_line
```

排除：

```text
max_bytes
```

原则：

> 纯 Delivery 参数不属于 Semantic Query Identity。

---

# 16. Context Entry Identity

Context Entry 完整身份包含：

```text
workspace

tool

semantic args

content

capture completeness
```

即：

```text
workspace_id
+
tool_name
+
args_hash
+
content_sha256
+
capture_truncated
```

`capture_truncated=false`

和：

```text
capture_truncated=true
```

不能因为正文前缀相同而被认为是同一条完整性事实。

---

# 17. Store Put

重复内容使用 UPSERT。

语义等价：

```sql
INSERT ...
ON CONFLICT (...) DO UPDATE SET
    last_accessed_at = :now,
    expires_at = :new_expiry;
```

禁止依赖：

```text
先 SELECT
再无条件 INSERT
```

处理重复结果。

也不使用：

```text
INSERT OR REPLACE
```

---

# 18. TTL

默认：

```text
ttlSeconds = 7200
```

即：

```text
2 hours
```

采用 Sliding TTL。

以下两种操作刷新：

```text
成功 Retrieve

重复 Put 命中已有 Entry
```

更新：

```text
last_accessed_at = now

expires_at = now + ttl
```

---

# 19. Cache Capacity

默认：

```text
maxStoreBytes = 268435456
```

即：

```text
256 MiB
```

容量不足时：

```text
1. 使用 expires_at 索引清理过期 Entry

2. 仍不足则按 last_accessed_at 淘汰 LRU Entry
```

清理采用：

```text
bounded batch
```

避免一次删除大量数据。

不需要额外后台清理任务。

---

# 20. SQLite 并发

v0.3 基于现有：

```text
SerenaDesktop 单实例
+
单 Broker 进程
```

假设。

ContextStore 使用：

```text
单 SQLite Connection

Mutex

spawn_blocking

WAL

bounded busy_timeout
```

Context Cache 不属于安全关键 Evidence。

因此不要求复制 Agent State Store 的全部安全持久化策略。

---

# 21. 本地缓存安全边界

Context Cache 可能包含源码、配置或其他 Workspace 内容。

v0.3 明确：

> Context Cache 是当前用户 AppData 下的临时明文本地缓存。

第一版：

```text
不做数据库加密

不做 .env / *.key 等路径黑名单

不做秘密内容自动识别
```

原因是简单路径黑名单无法可靠判断哪些源码或配置包含敏感信息。

但必须满足：

```text
默认 enabled=false

缓存有 TTL

缓存有容量上限

Context 内容不得写入 app.log / serena.log

retrievalId 不编码文件路径

UI 提供“清空上下文缓存”
```

Context Store 不提升任何读取权限。

它只能缓存：

> 当前 ActiveWorkspace 的 MCP Tool 已经有权限取得的 Captured Tool Text。

---

# 22. `context_retrieve`

第一版只支持：

```text
Range Mode
```

不支持：

```text
query
substring search
regex
semantic search
```

Schema：

```json
{
  "id": "ctx_xxx",
  "offset": 65536,
  "max_bytes": 32768
}
```

---

## 22.1 max_bytes

默认：

```text
32768
```

范围：

```text
1..131072
```

同样表示：

```text
UTF-8 byte ceiling
```

---

## 22.2 offset

`offset` 为：

```text
UTF-8 byte offset
```

必须满足：

```text
0 <= offset <= content_bytes
```

并且必须位于：

```text
UTF-8 char boundary
```

否则：

```text
INVALID_PARAMS
```

不进行 silent alignment。

---

## 22.3 返回

```json
{
  "workspace": {
    "id": "...",
    "name": "...",
    "root": "..."
  },

  "id": "ctx_xxx",

  "text": "...",

  "offset": 65536,

  "returnedBytes": 32765,

  "nextOffset": 98301,

  "totalCapturedBytes": 183420,

  "eof": false,

  "captureTruncated": false,

  "expiresAt": 1789000000000
}
```

翻页必须使用：

```text
nextOffset
```

不得自行：

```text
offset + text.length
```

因为字符长度不等于 UTF-8 byte 长度。

---

# 23. Tool 生命周期

`context_retrieve`：

```text
始终出现在 tools/list
```

不随 Context Optimization 开关动态增删。

这样保持 MCP Tool Schema 稳定。

---

## 23.1 Feature On

允许：

```text
创建新的 retrievalId
```

---

## 23.2 Feature Off

不再创建新的：

```text
retrievalId
```

但此前已经产生且仍有效的 Entry：

```text
仍可 context_retrieve
```

直到：

```text
TTL Expired

或

LRU Evicted
```

这样用户在对话中关闭优化不会立即破坏已经返回给 ChatGPT 的引用。

---

# 24. Retrieve Workspace 校验

每次 Retrieve 必须验证：

```text
当前存在 ActiveWorkspace

workspace_id 一致

canonical workspace root 一致
```

否则：

```text
CONTEXT_WORKSPACE_MISMATCH
```

Workspace：

```text
deactivate
```

后不得 Retrieve。

重新激活原 Workspace 后，在 Entry 未过期的情况下可以继续 Retrieve。

---

# 25. Context 错误

固定：

```text
CONTEXT_NOT_FOUND

CONTEXT_EXPIRED

CONTEXT_WORKSPACE_MISMATCH
```

参数错误使用：

```text
INVALID_PARAMS
```

错误继续使用 Broker 当前 MCP Tool Error 模式：

```text
isError=true
+
stable error code
```

不新增另一套 Error Envelope。

---

# 26. Cache Failure

必须保持 Source 和 Git 历史契约差异。

---

## Git

Context Store 写失败：

```text
返回当前 Git 行为
```

即：

```text
前缀
+
truncated=true
+
原有缩小查询 hint
```

没有：

```text
retrievalId
delivery
```

原因：

> Git 原本就允许不可恢复 truncated success。

---

## Source

如果：

```text
capturedBytes > inlineBytes
```

但 Context Store 写失败：

```text
OUTPUT_LIMIT_EXCEEDED
```

不得返回不可恢复 Partial Success。

原因：

> Source 原本不允许不可恢复 partial success。

---

# 27. Workspace Lock

Source / Git Backend 调用仍然保持现有 Workspace Read Lock。

Backend Result 获取完成后：

```text
clone workspace identity

drop Workspace Read Lock
```

然后才执行：

```text
UTF-8 clamp

Context Store

Result Envelope
```

Context Optimization 的收益只是：

> 不额外延长 Workspace Lock 临界区。

不把它描述为 Workspace 切换阻塞问题的主要解决方案。

---

# 28. 配置

```json
{
  "contextOptimization": {
    "enabled": false,
    "ttlSeconds": 7200,
    "maxStoreBytes": 268435456
  }
}
```

第一版只允许 UI 配置：

```text
上下文优化
[开 / 关]

缓存大小
xx MB

[清空上下文缓存]
```

不暴露：

```text
Capture Limit

Backend Char Limit

Inline Default

SQLite 参数
```

这些保持代码内固定。

---

# 29. Metrics

为了判断实验是否值得继续，只记录：

```text
Captured Bytes

Inline Delivered Bytes

Retrieved Bytes

Total Delivered Bytes

Retrieve Count
```

其中：

```text
Total Delivered Bytes
=
Inline Delivered Bytes
+
Retrieved Bytes
```

不记录：

```text
Saved Tokens
```

不显示：

```text
Compression Ratio
deliveryRatio
```

不直接宣称：

```text
节省 X MB
```

因为 Retrieve 后的实际交付量才是有效指标。

---

# 30. 日志

允许记录：

```text
tool name

capturedBytes

returnedBytes

captureTruncated

cache hit/miss

retrieve bytes

error code
```

禁止记录：

```text
Captured Content

Retrieve Text

文件内容

Diff 内容

Secret 内容
```

普通诊断也不得把 Context BLOB 输出到日志。

---

# 31. Serena Contract Test

v0.3 实现 Source 前必须针对 SerenaDesktop 实际固定的 Serena 版本验证：

```text
source_read_file

max_answer_chars = 131072
```

至少覆盖：

```text
ASCII source

中文 source

Emoji / Unicode source

超过 max_answer_chars
```

记录真实行为。

如果固定 Serena 版本实际行为和本文明显不一致：

```text
停止 Source 部分实现

记录 Material Contract Difference
```

Git Phase 不受影响。

---

# 32. 实施顺序

## Phase 1 — Context Store + Retrieve

先实现：

```text
context-cache.db

ContextStore

TTL

Capacity / LRU

UPSERT

context_retrieve

Workspace Validation

Registry / Schema
```

不接 Source / Git。

---

## Phase 2 — Git Retrievable Overflow

接入：

```text
git_diff

git_show
```

实现：

```text
Inline / Capture 分离

captureTruncated

retrievalId

Git cache failure fallback
```

这是第一条真实产品验证路径。

---

## Phase 3 — Source Read File

先跑：

```text
Serena Contract Test
```

再接：

```text
source_read_file
```

实现：

```text
max_answer_chars = 131072

UTF-8 byte clamp

Source Error → Retrievable Partial

Source cache failure → OUTPUT_LIMIT_EXCEEDED
```

Source 属于显式行为契约变化，应单独 Review。

---

## Phase 4 — Real ChatGPT Evaluation

打开：

```text
contextOptimization.enabled=true
```

实际使用一段时间。

比较：

```text
Baseline

vs

Context Optimization
```

重点观察：

```text
大型 Diff 是否减少首次上下文输入

ChatGPT 是否会在需要时 Retrieve

是否把 Partial Result 错认为完整结果

Review / Bug 定位质量是否下降

Retrieve 次数

额外 Round Trip

总体交付 Bytes

使用体验
```

第一版不预设复杂数学阈值。

如果实际效果不好：

```text
保持默认关闭
```

即可。

---

# 33. Default Enable Gate

v0.3 发布时：

```text
enabled=false
```

功能是否未来默认开启，只根据真实 ChatGPT 使用结果决定。

至少要求：

```text
没有发现稳定复现的
“因为 Partial Result 被误认为完整结果而造成严重错误”

Review / 定位质量没有明显下降

Total Delivered Bytes 相比基线确实有下降趋势

Retrieve Round Trip 没有明显破坏使用体验
```

如果不满足：

```text
功能仍可作为实验开关存在
```

不继续扩大范围。

---

# 34. v0.3 核心不变量

必须保持：

```text
1.
Feature Off 时现有 Tool Contract 不变。


2.
max_bytes 永远表示 Inline UTF-8 byte ceiling。


3.
Context Store 保存 Captured Tool Text，
不宣称保存 Backend Raw Bytes。


4.
Git 与 Source 的 Cache Failure
保持各自历史契约。


5.
Source 只有存在可恢复 Overflow
且 Cache 成功时才能返回 Partial Success。


6.
Source/Git Capture 超限可以保存有界前缀，
并明确 captureTruncated=true。


7.
captureTruncated=true
不得描述为完整 Backend Result。


8.
只有 capturedBytes > returnedBytes
才创建 retrievalId。


9.
context_retrieve 使用 UTF-8 byte offset。


10.
nextOffset 是标准分页位置。


11.
Context Entry 绑定 Workspace。


12.
Deactivate 后不可 Retrieve。


13.
Feature Off 后旧 retrievalId
仍可读取到 TTL / Eviction。


14.
Context Cache 内容不得进入日志。


15.
Context Cache 有 TTL 和硬容量上限。


16.
Optimizer 不在 Workspace Read Lock 内执行 SQLite。


17.
Context Optimization 完全旁路 Agent。


18.
所有生产数据读取仍然有界。


19.
Full Result 不增加 delivery Envelope。


20.
v0.3 不做任何语义压缩或采样。
```

---

# 35. 验收

## Feature Off

验证：

```text
source_read_file

git_diff

git_show
```

行为与当前版本一致。

---

## Git

验证：

```text
小结果

64 KiB ~ 256 KiB

> 256 KiB

显式 max_bytes

Cache Failure

Unicode Diff
```

必须正确区分：

```text
truncated

captureTruncated

retrievalId
```

---

## Source

验证：

```text
小于 Inline

Inline ~ Capture

UTF-8 bytes 超 Capture

Backend max_answer_chars 超限

显式 max_bytes=131072

Cache Failure
```

---

## Retrieve

验证：

```text
第一页

多页

eof

UTF-8 nextOffset

非法 byte boundary

TTL

LRU

Feature Off 后旧 ID

Workspace Switch

Workspace Deactivate
```

---

## Store

验证：

```text
重复 Put UPSERT

TTL Refresh

Retrieve TTL Refresh

Capacity Eviction

captureTruncated Identity

并发 Tool Call
```

---

## Isolation

验证：

```text
Agent Tool

Agent Runtime

agent-state.db

Workspace Claim
```

没有任何行为变化。

---

## Engineering Gate

至少：

```text
cargo test

cargo clippy --all-targets -- -D warnings

Frontend lint / typecheck

MCP Contract Test

真实 ChatGPT Smoke
```

通过。

---

# 36. 后续是否继续

v0.3 完成后不自动进入下一阶段。

只有真实结果证明：

```text
Retrievable Overflow
```

确实能：

```text
降低 Context Delivery

且

不明显降低任务质量
```

才考虑后续：

```text
Structured Compression

Search / List Dedup

Relevance Sampling

context_retrieve Query Mode

CodeGraph Integration

Cross-turn Dedup
```

这些不属于 v0.3。

---

# 37. 最终定位

第一版只实现：

```text
                 ChatGPT
                    │
                    ▼
               MCP Broker
                    │
          ┌─────────┴─────────┐
          │                   │
   Normal Tool Result    Overflow Result
                              │
                              ▼
                      context-cache.db
                              │
                              ▼
                      context_retrieve
```

核心不是：

```text
智能压缩所有 Tool Result
```

而是先验证一个更基础的能力：

> **Context Window 只承载当前需要的 Working Set，本地 Broker 保存已经捕获但暂时不需要进入上下文的 Overflow。**

如果这个最小模型在真实 ChatGPT 工作流中有效，再增加更复杂的压缩和检索算法。

Technical Design Status：

```text
READY FOR IMPLEMENTATION REVIEW
```
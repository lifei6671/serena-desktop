# Stage B Usage Host Freeze — CodeBuddy 2.158.0

Stage B Host Gate **PASS**。能力建议提升为 **supported but completeness-aware**：Provider 可提供 execution-scoped token telemetry；这不是生产实现验收，也不证明本次每个 Execution 的完整总量。Stage A 已冻结；Crash 未执行；CB5-005 仍 in_progress。

## 1. Observed wire evidence

父 Host 提供 CommandRun `command-26872-1790480352549345-80`：timeoutMs=420000，completed / exitCode=0 / timedOut=false，stdout scenario=usage-repair,status=PASS。本轮只读复核落盘证据，没有重新调用 Provider；CommandRun 元数据来源为父 Host 报告。

- [result](evidence/usage/usage-repair.result.json)：SHA256 `985faa29686b5df3649af4f017ee75673a57a12e21df7d7d22aedaee39117a5e`。
- [analysis](evidence/usage/usage-repair-analysis.json)：SHA256 `6a0efe69fd2137e93e9a826947df0a22d6b6ef5280e228563c757424eaa887da`。
- 两份 raw evidence 原字节保留；analysis 的 `token_usage=false` / `publicUsage=unknown` 是冻结前保守判定，不改写。本文件提供其要求的 Host 语义冻结，不能反向改变 raw observation 的归因等级。
- exact S1=`01a0e0f2-0af3-7696-bce7-95ac5a8edc68`，R2 使用 session/resume，workspaceDelta=[]，manifestComplete=true，workspaceDeleted=true；两个 owned child 均 reap/cleanup 成功。不是 Windows Job/tree containment 证据。

末帧数值与 ordering（sequence 在各 Runtime 内计数；250ms late window）：

| Turn | Runtime | usage → terminal sequence | prompt_tokens | completion_tokens | total_tokens | lateUsage |
|---|---|---|---:|---:|---:|---|
| P1 | R1 | 55 → 57 | 25708 | 105 | 25813 | [] |
| P2 | R1 | 96 → 98 | 25859 | 81 | 25940 | [] |
| P3 | R2 | 115 → 117 | 25986 | 302 | 26288 | [] |

三者 total=prompt+completion；completion 105→81→302 非单调，与 Session cumulative counter 不符。这里不把三个 final frame 或单帧 total 声称为 Execution aggregate。窗口外 late 行为未证明。

R1 P2 最后 used=25859,size=1000000；R2 resume 首帧同值。所有观测 size 均为1000000。used/size 冻结为 context occupancy/window gauge；不能据此推导累计 token，不能把 reset 零值自动覆盖最后有效正值；本次 restart 同值不证明所有 restart 永不 reset。

每个 final breakdown frame 都含两个 string 类型的 hashed field path：

- SHA256(`codebuddy.ai/requestId`)=`a4e516ff8711756eb2ecc5782e8fca63812e0e4fd823e72d367fe76635fd61e6`。
- SHA256(`codebuddy.ai/messageId`)=`3e671125ccaf116a96c951412b407e0660d0c5f9d382355b73a45abe8c20190d`。

这验证键的存在和类型，**没有保存字符串值，不能证明 requestId 的 exact equality、messageId 唯一集合或覆盖完整**。raw placement 仍为 window_only；prompt terminal 的 exact RPC correlation 不自动传递给 usage。`conversationRequestId` 与 `requestId` 不能仅凭名字视为同一身份。

三个 terminal PromptResponse 的安全投影均只有 /_meta object，没有可消费的 modelUsage。costFields=[]，不能从本次 wire 冻结 cost 归属。显式 cache_read_input_tokens/cache_creation_input_tokens 在三个末帧均为0；另有 nested cached_tokens，不能混合相加或猜测为 cache read/write。未冻结的 reasoning 等字段保持 null。

## 2. Pinned 2.158.0 implementation evidence

只读本机 package.json 确认 version=2.158.0；bundle 路径为 `C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\dist\codebuddy-lite-wb.mjs`，9017499 bytes，SHA256 `980f5c2e6652217548115e426fbbf20c5558b3b14d6019a5f2c9f9aa85abd2c0`。定位用行号和 anchor，机器可读 byte offsets 见 [checks](usage-host-freeze-checks.json)；不复制 bundle 正文到 task。

1. line535 AcpUsagePublisher：Eu 固定为 codebuddy.ai/requestId，Ep 固定为 codebuddy.ai/messageId，与 wire hashed paths 一致。
2. doPublish 调 tryBuildUsageDetail(stream events) 得到 R；W[Eu]=N，W[Ep]=M（存在时），W.usage=R.usage。精确细节：M=R?.messageId ?? session.messageId，存在 fallback；不能把 fallback 自动视为已验证 model-call ID。usage 是选中的 input/model event 原始 usage，Publisher 没有将其累加成 Session/Execution 总量。
3. line535 readModelUsage 读取 response_done.response.usage 或 model.event.usage；getInputTokensFromUsage 读取 inputTokens/input_tokens/prompt_tokens，要求 finite、非负。
4. line525 prompt finally 为当前 conversationRequestId 构造 carrier，调用并 await publish(sessionId,"prompt_end",carrier)，finally 完成后 prompt RPC 才完成。line535 non-stream publish 清掉 stream timer、splice pending streamEvents，并经 publishChain 串行 enqueue。wire 中三个 final usage 均在 terminal 前，与该边界一致。
5. scheduleStreamPublish 在 event 到达时捕获 getRootRequestId(session) ?? conversationRequestId；pending batch requestId 与新 ID 不同则先按旧 ID/carrier flush，再放入新 event。timer flush 使用捕获的 batch identity，不静默把旧 batch 归给新 Turn。
6. line1741 CostService.recordTokenUsage 确有按 model 累加 input/output/cachedRead/cachedWrite 的能力；line503 enrichResultWithUsage/buildModelUsage 可从内部 CostService 生成 result.modelUsage。内部 result enrichment 不等于 ACP PromptResponse 对外暴露；当前 terminal wire 未提供可消费字段，SerenaDesktop 不得依赖它。

边界限制：tryBuildUsageDetail 从 batch 逆序选择候选，正 cost 候选优先；不是逐 model-call 全量账本。doPublish 还有相同 gauge/cost/category 的抑制路径；sendSessionUpdate 错误被 catch，prompt finally 的 publish 异常也被 catch。故 await flush 表示 pinned 实现的排队/等待边界，**不保证每个 call 都发送成功或覆盖全部 calls**。以上只冻结本机 2.158.0，未来版本必须重新 probe。

## 3. Inferred / approved product mapping

建议 capability 从冻结前 false 提升为 **supported but completeness-aware**，含义为可提供 execution-scoped token telemetry，而非每次执行必有 complete token total。尚未修改生产 capability。

- 只消费实际 provider requestId 精确绑定到当前 Execution/Turn 的 usage；记录 Session/Runtime ownership，拒绝错身份。不能仅按接收窗口归因。
- 以 provider messageId 为 model-call 去重键。同 ID 多帧只保留最新有效 usage；不同 ID 才按 model call 聚合。每 call 的 prompt_tokens/completion_tokens/total_tokens 是该调用 usage；Execution aggregate 为当前 requestId 内 unique messageId 集合各字段之和，不能把单帧 total 当整个 Execution 总量。
- cached read/write 仅映射 Provider 明确字段；未冻结字段保持 null。不得用 used/size、context category、cost、文本长度或 used delta 推导 tokens，也不填造缺失字段。
- messageId 缺失、requestId 不匹配、数字非法/回退、不能证明去重/覆盖完整时 completeness 只能 partial/unknown。保留已有有效观测和异常状态，不以坏帧覆盖或伪造 complete；“最新有效”不能掩盖异常。
- 不做 Codex 式 Session cumulative baseline subtraction。Continue E2 只计算 E2 自己 exact requestId 下的 call 集合，无跨 Runtime baseline，相同 call 帧不重复计数。
- terminal 前 flush 是完成性判断的一个条件，不是充分条件。本次没有保存 identity 值，且未验证多 call 覆盖，因此不能宣布 Execution aggregate 已完整验证。

生产前置条件：在后续明确授权任务中实现 requestId 实值绑定、可靠 messageId call identity、去重覆盖/聚合、数字校验、completeness 与 provenance；验证多 call/重复帧/缺 ID/错 ID/回退/丢帧或发送失败/late/restart 等边界，证明覆盖不足时降级；基于 pinned 版本完成生产 Host 验收。当前仅文档契约冻结，无 DB、Provider/Runtime 或生产源码变更，无 Crash PASS，未完成整张 CB5-005。

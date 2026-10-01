# Stage C Crash / Result Recovery design

状态：设计/实现范围冻结，真实CLI NOT_RUN。Stage A/B Host PASS原字节保留。

两个独立mode为crash-before-terminal、crash-after-terminal；每次fresh ordinary Win32 temp workspace/new Session。共用SDK 2.2.0 typed initialize/new/prompt/load、既有有界Tee/cleanup/manifest。只增加Stage C模块和main入口；不改变Stage A/B协议路径。

R1在发prompt前以create_new+fsync持久化session/conversation/request identity；prompt正文仅内存。requestId与conversationRequestId显式设置同一随机UUIDv7值，仍分别校验Provider回显。before只有收到exact Session/requestId activity且当前接收帧中没有exact RPC terminal才触发owned-child kill；terminal先到则NOT_OBSERVED，不重试。after等待exact RPC、end_turn和exact conversationRequestId，随后立即cleanup，不先写terminal/result文件。live摘要在cleanup后从内存wire计算，用于模拟未持久化生产结果的对照；不存在StateStore调用。

R2仅typed session/load exact S1/cwd，接收早到history，不发送任何prompt，不调用resume。每帧验证typed session envelope；history requestId必须exact匹配，messageId仅hash/equality。按messageId首次出现顺序分组assistant text，同组相同文本片段去重、不同片段按顺序拼接；去重仅诊断规则，不能证明覆盖完整，不丢弃跨messageId重复文本。live流按chunk拼接不按文本去重。所有正文只在有界内存，落盘仅hash/UTF-8 length。

resultCompleteness最多partial，有绑定history但无旧terminal为partial，无可归因history为unknown。即使after replay hash等于live也不恢复business completed。任何疑似旧terminal/stopReason wire都记录Material Contract Difference且不升级。scenario PASS只表示目标窗口/检查/cleanup被观测完成，可伴随resultCompleteness=partial；未观测窗口为NOT_OBSERVED；协议、identity、load、manifest、cleanup或after答案校验不满足为PARTIAL。

§23安全分离：所有报告固定windowsJobAtCreationProven=false、runtimeTerminationEvidenceProven=false、claimReleasePermitted=false。R2不能证明R1消失。生产旧Job evidence未知时unknown+Claim retained；独立取得R1 Job evidence后可按generic runtime-termination recovery收敛interrupted，与结果partial/unknown不冲突。

固定evidence/crash/<mode>.attempt-started.json、<mode>.prompt-identity.json、<mode>.result.json。无force/output/retry，残留任何文件拒绝新attempt；两个mode互不复用。R1 120s、R2 120s，each cleanup最多7s，history drain250ms包含R2预算；总异步预算254s，外层推荐300000ms或420000ms。本地同步文件IO不是硬实时预算。

验证：新增fake进程覆盖两窗口、错identity、重复history、无terminal、load失败/无fallback、no R2 prompt、sentinel与cleanup；复用现有manifest/frame边界tests作必要回归。独立只读review后停Host Gate。不实现migration或生产代码。

## Pinned实现参考（不替代Stage C Host）

只读复核本机2.158.0 bundle SHA256 `980f5c2e6652217548115e426fbbf20c5558b3b14d6019a5f2c9f9aa85abd2c0`：line525 loadSession先initializeSession(type=load)，有history时replayHistory；resumeSession/unstable_resumeSession没有replayHistory。getReplayRequestId读取providerData.conversationRequestId ?? requestId；collectSessionReplayEvents在缺字段时补回codebuddy.ai/requestId，assistant/reasoning从历史message identity补codebuddy.ai/messageId。emitSessionReplayEvents调用sessionUpdate/extNotification，而非旧prompt RPC response。这支持load作为独立Result Recovery inspection候选，不支持将其作为Continue fallback或恢复completed。所有未来版本需重新probe。

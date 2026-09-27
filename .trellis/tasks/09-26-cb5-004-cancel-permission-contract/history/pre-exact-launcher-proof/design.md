# CB5-004 Design

Identity：cancel / permission 必须绑定 harness-owned runtime、exact sessionId 与当前 prompt correlation。identity mismatch 一律 fail closed。

Cancel timing：before-side-effect 不能靠固定 sleep 猜测，必须同时看到 Prompt 已进入运行态和 manifest 仍无变化；after-side-effect 必须由 marker 文件实际存在/hash正确触发。

Permission handler：使用 official agent-client-protocol 2.2.0 typed RequestPermissionRequest/response。记录实际 option id/kind；拒绝按 typed enum/kind选择，不解析 label。Responder只能使用一次。

Terminal convergence：分别记录 cancel request sent、prompt terminal received、stopReason、runtime terminated。只有 Prompt terminal 是 Provider terminal evidence；Runtime termination 是 Serena safety evidence。

Side effects：所有文件变化只允许发生在 temp root。证据捕获后删除整个 temp root，不为了“恢复干净”而改写真实 delta。
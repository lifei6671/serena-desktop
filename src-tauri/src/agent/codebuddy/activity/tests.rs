//! typed 分类、身份和无文本公共事件的契约测试。
use super::*;
use crate::agent::{
    activity::ActivityPhase,
    codebuddy::protocol::{Limits, Shared},
};
use serde_json::json;

/// 通过协议 route 创建真实 SessionFrame，不自造内部 frame 元数据。
fn frame(mut update: Value) -> SessionFrame {
    if update.get("_meta").is_none() {
        update["_meta"] = json!({"codebuddy.ai/conversationRequestId":"c"});
    }
    let shared = Shared::new(Limits::default());
    shared.register_route("s").unwrap();
    shared
        .notification(
            "session/update".into(),
            json!({"sessionId":"s","update":update}),
        )
        .unwrap();
    shared.take_session("s").unwrap().pop().unwrap()
}
/// 构造只读 exact 身份和小容量缓存。
fn mapper() -> ActivityMapper {
    ActivityMapper::new("e".into(), "s".into(), "c".into(), Some("r".into()), 2)
}
/// 工具标题与输入故意包含误导文字，分类只能依据 kind。
fn tool(kind: &str, title: &str) -> Value {
    json!({"sessionUpdate":"tool_call","toolCallId":"t","title":title,"kind":kind,"status":"in_progress","rawInput":{"command":"cargo test"},"rawOutput":"secret stdout"})
}

#[test]
/// 覆盖全部结构化 kind、未来 kind，以及禁止文本推断的快照。
fn kinds_and_public_snapshot_are_text_free() {
    for (kind, expected) in [
        ("read", ToolCategory::Read),
        ("edit", ToolCategory::Edit),
        ("delete", ToolCategory::Edit),
        ("move", ToolCategory::Edit),
        ("execute", ToolCategory::Command),
        ("other", ToolCategory::Tool),
        ("search", ToolCategory::Tool),
        ("fetch", ToolCategory::Tool),
        ("think", ToolCategory::Tool),
        ("switch_mode", ToolCategory::Tool),
        ("future", ToolCategory::Tool),
    ] {
        for title in ["Read", "cargo test", "cargo build"] {
            let start = now();
            let event = mapper().map(&frame(tool(kind, title))).unwrap();
            assert_eq!(event.phase(), ActivityPhase::Tool);
            assert_eq!(event.tool_category(), Some(expected));
            assert!(event.observed_at() >= start && event.observed_at() <= now());
            assert_eq!(
                format!("{event:?}"),
                format!(
                    "AgentActivityEvent {{ execution_id: \"e\", phase: Tool, tool_category: Some({expected:?}), observed_at: {} }}",
                    event.observed_at()
                )
            );
        }
    }
}
#[test]
/// 初始工具状态决定 phase；所有状态都记忆结构化 kind，标题不能推断 Test/Build。
fn initial_tool_status_preserves_category_for_followup() {
    for status in ["pending", "in_progress", "completed", "failed"] {
        for (kind, title, expected) in [
            ("read", "secret", ToolCategory::Read),
            ("execute", "cargo test", ToolCategory::Command),
            ("execute", "cargo build", ToolCategory::Command),
            ("other", "Read", ToolCategory::Tool),
        ] {
            let mut map = mapper();
            let mut initial = tool(kind, title);
            initial["status"] = json!(status);
            let event = map.map(&frame(initial)).unwrap();
            if matches!(status, "completed" | "failed") {
                assert_eq!(event.phase(), ActivityPhase::Provider);
                assert_eq!(event.tool_category(), None);
            } else {
                assert_eq!(event.phase(), ActivityPhase::Tool);
                assert_eq!(event.tool_category(), Some(expected));
            }
            let followup = map
                .map(&frame(json!({
                    "sessionUpdate":"tool_call_update","toolCallId":"t","status":"pending"
                })))
                .unwrap();
            assert_eq!(followup.phase(), ActivityPhase::Tool);
            assert_eq!(followup.tool_category(), Some(expected));
        }
    }
}

#[test]
/// 增量只继承同 id；状态缺失不 spam，缓存有界，完成返回 Provider。
fn lifecycle_incremental_and_bounded_memory() {
    let mut map = mapper();
    assert!(
        map.map(&frame(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"unknown","status":"pending"})
        ))
        .is_none()
    );
    map.map(&frame(tool("read", "secret"))).unwrap();
    for (kind, expected) in [
        (Value::Null, ToolCategory::Read),
        (json!("execute"), ToolCategory::Command),
    ] {
        let mut value =
            json!({"sessionUpdate":"tool_call_update","toolCallId":"t","status":"in_progress"});
        if !kind.is_null() {
            value["kind"] = kind;
        }
        assert_eq!(
            map.map(&frame(value)).unwrap().tool_category(),
            Some(expected)
        );
    }
    for status in ["completed", "failed"] {
        assert_eq!(
            map.map(&frame(
                json!({"sessionUpdate":"tool_call_update","toolCallId":"t","status":status})
            ))
            .unwrap()
            .phase(),
            ActivityPhase::Provider
        );
    }
    assert!(map.map(&frame(json!({"sessionUpdate":"tool_call_update","toolCallId":"t","rawInput":"secret","rawOutput":"secret"}))).is_none());
    assert!(
        map.map(&frame(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"t","kind":"edit"})
        ))
        .is_none()
    );
    assert_eq!(
        map.map(&frame(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"t","status":"pending"})
        ))
        .unwrap()
        .tool_category(),
        Some(ToolCategory::Edit)
    );
    for i in 0..10 {
        let mut value = tool("read", "secret");
        value["toolCallId"] = json!(i.to_string());
        map.map(&frame(value));
    }
    assert_eq!(map.tools.len(), 2);
    assert!(
        map.map(&frame(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"9","status":"pending"})
        ))
        .is_none()
    );
}
#[test]
/// 身份缺失、错配、畸形及未知 variant 一律不发布；不借用 envelope meta。
fn identity_and_parse_failures_drop() {
    for meta in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"codebuddy.ai/conversationRequestId":42}),
        json!({"codebuddy.ai/conversationRequestId":"wrong"}),
        json!({"codebuddy.ai/conversationRequestId":"c","codebuddy.ai/requestId":"wrong"}),
        json!({"codebuddy.ai/conversationRequestId":"c","codebuddy.ai/requestId":42}),
    ] {
        let mut value = tool("read", "secret");
        value["_meta"] = meta;
        assert!(mapper().map(&frame(value)).is_none());
    }
    let mut foreign = frame(tool("read", "secret"));
    foreign.session_id = "foreign".into();
    assert!(mapper().map(&foreign).is_none());
    for update in [
        json!({"sessionUpdate":"future"}),
        json!({"sessionUpdate":"tool_call"}),
        json!({"sessionUpdate":"tool_call_update","toolCallId":"t","status":42}),
        json!({"sessionUpdate":"usage_update","used":10,"size":100}),
        json!({"sessionUpdate":"available_commands_update","availableCommands":[]}),
    ] {
        assert!(mapper().map(&frame(update)).is_none());
    }
    let mut map = mapper();
    map.request = None;
    let mut value = tool("read", "secret");
    value["_meta"] =
        json!({"codebuddy.ai/conversationRequestId":"c","codebuddy.ai/requestId":"untrusted"});
    assert!(map.map(&frame(value)).is_some());
    assert!(map.request.is_none());
}
#[test]
/// 消息和思考只表示 Provider processing，伪终态字段没有 authority。
fn messages_and_forged_terminal_are_provider_only() {
    for kind in ["agent_message_chunk", "agent_thought_chunk"] {
        let event=mapper().map(&frame(json!({"sessionUpdate":kind,"content":{"type":"text","text":"sensitive command stdout"},"stopReason":"cancelled","terminal":true,"status":"completed","observedAt":-1}))).unwrap();
        assert_eq!(event.phase(), ActivityPhase::Provider);
        assert_eq!(event.tool_category(), None);
        assert!(!format!("{event:?}").contains("sensitive"));
        assert!(event.observed_at() > 0);
    }
}

#[test]
/// CB5-003 sanitized Host 摘录必须在 pinned SDK 下保留真实 kind/meta。
fn host_structured_fixture_retains_kind_and_identity() {
    let values: Vec<Value> =
        include_str!("../../../../tests/fixtures/codebuddy_activity_host.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    let session = values[0]["sessionId"].as_str().unwrap();
    let conversation = values[0]["update"]["_meta"]["codebuddy.ai/conversationRequestId"]
        .as_str()
        .unwrap();
    let mut map = ActivityMapper::new(
        "e".into(),
        session.into(),
        conversation.into(),
        Some(conversation.into()),
        4,
    );
    let shared = Shared::new(Limits::default());
    shared.register_route(session).unwrap();
    for value in &values {
        shared
            .notification("session/update".into(), value.clone())
            .unwrap();
    }
    let result = shared
        .take_session(session)
        .unwrap()
        .iter()
        .map(|frame| map.map(frame).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        result
            .iter()
            .map(|event| (event.phase(), event.tool_category()))
            .collect::<Vec<_>>(),
        vec![
            (ActivityPhase::Tool, Some(ToolCategory::Tool)),
            (ActivityPhase::Tool, Some(ToolCategory::Read)),
            (ActivityPhase::Provider, None)
        ]
    );
}

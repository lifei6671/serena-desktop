use super::*;
use serde_json::json;

fn input() -> CreateExecutionInput {
    serde_json::from_value(json!({
        "agent_id":"a", "request_key":"k", "prompt":"hello", "execution_profile":{},
        "workspace_id":"w", "canonical_workspace_root":"C:/workspace", "mode":"read_only"
    }))
    .unwrap()
}

#[test]
fn v3_general_and_testing_fixed_bytes_and_sha256_vectors() {
    let request = canonicalize_request(input()).unwrap();
    assert_eq!(request.input().task_role, AgentTaskRole::General);
    assert_eq!(request.input().provider.as_str(), "codex");
    assert_eq!(
        std::str::from_utf8(request.bytes()).unwrap(),
        r#"["execution-request-v3","a","k","hello","{}","w","C:/workspace",1,"codex","read_only",null,"general"]"#
    );
    assert_eq!(
        request.request_hash(),
        "4ba6e8207d43b5c7ff9257a8023060febf2f81ee165f6852654d03ef5111ef9c"
    );
    for _ in 0..20 {
        let repeated = canonicalize_request(input()).unwrap();
        assert_eq!(repeated.bytes(), request.bytes());
        assert_eq!(repeated.request_hash(), request.request_hash());
    }
    let mut testing = input();
    testing.task_role = AgentTaskRole::Testing;
    let testing = canonicalize_request(testing).unwrap();
    assert_eq!(
        std::str::from_utf8(testing.bytes()).unwrap(),
        r#"["execution-request-v3","a","k","hello","{}","w","C:/workspace",1,"codex","read_only",null,"testing"]"#
    );
    assert_eq!(
        testing.request_hash(),
        "2ce8501c178bae31be423833bf891247dc74a7528746f61af3781728f2f83b4b"
    );
    assert_ne!(testing.request_hash(), request.request_hash());
}

/// v2 历史向量固定，新增角色不能改变旧行的重试身份。
#[test]
fn legacy_v2_general_hash_keeps_frozen_vector() {
    assert_eq!(
        legacy_v2_request_hash(&input()).unwrap(),
        "9aa4fddbd7a43e9dc13919e7259518536eb24109b7c268a73a28770e058cc860"
    );
}

/// Fake ProviderId 经创建 DTO 往返后保持原文，v3 tuple 仍使用 v2 位置与 wire string。
#[test]
fn fake_provider_id_round_trips_through_creation_and_v3_bytes() {
    let mut value = json!({
        "agent_id":"a", "request_key":"k", "prompt":"hello", "execution_profile":{},
        "workspace_id":"w", "canonical_workspace_root":"C:/workspace", "mode":"read_only"
    });
    value["provider"] = json!("fake.provider-v1");
    let fake_input: CreateExecutionInput = serde_json::from_value(value).unwrap();
    assert_eq!(fake_input.provider.as_str(), "fake.provider-v1");
    let wire = serde_json::to_value(&fake_input.provider).unwrap();
    assert_eq!(wire, json!("fake.provider-v1"));
    assert_eq!(
        serde_json::from_value::<ProviderId>(wire).unwrap(),
        fake_input.provider
    );
    let request = canonicalize_request(fake_input).unwrap();
    assert_eq!(
        std::str::from_utf8(request.bytes()).unwrap(),
        r#"["execution-request-v3","a","k","hello","{}","w","C:/workspace",1,"fake.provider-v1","read_only",null,"general"]"#
    );
    assert_eq!(
        request.request_hash(),
        "d90c7ce3c895c7358323d8ce817b98b7af4f4124374e19423f3b35373bc5bcdf"
    );
    assert_ne!(
        request.request_hash(),
        canonicalize_request(input()).unwrap().request_hash()
    );
}

/// 创建 DTO 与 ProviderId 构造器一致拒绝非法 identity，不回退为 Codex。
#[test]
fn creation_rejects_invalid_provider_id_without_fallback() {
    for invalid in [
        "",
        " codex",
        "codex ",
        "co dex",
        "co\t-dex",
        "co\n-dex",
        "co\u{0007}dex",
        "co\u{00a0}dex",
    ] {
        assert!(ProviderId::new(invalid.into()).is_err(), "{invalid:?}");
        let mut value = json!({
            "agent_id":"a", "request_key":"k", "prompt":"hello", "execution_profile":{},
            "workspace_id":"w", "canonical_workspace_root":"C:/workspace", "mode":"read_only"
        });
        value["provider"] = json!(invalid);
        assert!(
            serde_json::from_value::<CreateExecutionInput>(value).is_err(),
            "{invalid:?}"
        );
    }
}

/// 固定角色的 wire 值必须完整往返，不接受别名或大小写归一。
#[test]
fn task_role_serde_round_trip_and_invalid_values() {
    for (role, wire) in [
        (AgentTaskRole::Development, "development"),
        (AgentTaskRole::Testing, "testing"),
        (AgentTaskRole::Review, "review"),
        (AgentTaskRole::Analysis, "analysis"),
        (AgentTaskRole::General, "general"),
    ] {
        assert_eq!(role.as_str(), wire);
        assert_eq!(serde_json::to_value(role).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<AgentTaskRole>(json!(wire)).unwrap(),
            role
        );
    }
    for invalid in [
        json!("Development"),
        json!("unknown"),
        json!(""),
        json!(null),
        json!(1),
    ] {
        assert!(serde_json::from_value::<AgentTaskRole>(invalid.clone()).is_err());
        let mut legacy = json!({
            "agent_id":"a", "request_key":"k", "prompt":"hello", "execution_profile":{},
            "workspace_id":"w", "canonical_workspace_root":"C:/workspace", "mode":"read_only"
        });
        legacy["task_role"] = invalid;
        assert!(serde_json::from_value::<CreateExecutionInput>(legacy).is_err());
    }
}

/// 未携带角色与显式 General 得到相同 v3 identity。
#[test]
fn legacy_input_defaults_to_general_v3_identity() {
    let legacy = input();
    assert_eq!(legacy.task_role, AgentTaskRole::General);
    assert_eq!(AgentTaskRole::default(), AgentTaskRole::General);
    let mut explicit = json!({
        "agent_id":"a", "request_key":"k", "prompt":"hello", "execution_profile":{},
        "workspace_id":"w", "canonical_workspace_root":"C:/workspace", "mode":"read_only"
    });
    explicit["task_role"] = json!("general");
    let explicit: CreateExecutionInput = serde_json::from_value(explicit).unwrap();
    assert_eq!(
        canonicalize_request(legacy).unwrap().bytes(),
        canonicalize_request(explicit).unwrap().bytes()
    );
}

#[test]
fn field_order_and_explicit_defaults_are_equivalent() {
    let explicit: CreateExecutionInput = serde_json::from_str(
        r#"{
        "thread_id":null,"mode":"read_only","provider":"codex",
        "canonical_workspace_root":"C:/workspace","workspace_id":"w",
        "execution_profile":{},"prompt":"hello","request_key":"k","agent_id":"a"
    }"#,
    )
    .unwrap();
    assert_eq!(
        canonicalize_request(input()).unwrap().bytes(),
        canonicalize_request(explicit).unwrap().bytes()
    );
}

#[test]
fn legacy_v1_hash_excludes_generation_while_current_v3_hash_includes_it() {
    let generation_one = input();
    let mut generation_two = generation_one.clone();
    generation_two.workspace_generation = 2;
    assert_eq!(
        legacy_pre_workspace_generation_hash(&generation_one).unwrap(),
        legacy_pre_workspace_generation_hash(&generation_two).unwrap()
    );
    assert_ne!(
        canonicalize_request(generation_one).unwrap().request_hash(),
        canonicalize_request(generation_two).unwrap().request_hash()
    );
}

#[test]
fn nested_objects_sort_keys_but_arrays_preserve_order() {
    let mut a = input();
    a.execution_profile =
        serde_json::from_str(r#"{"z":[{"b":2,"a":1},false],"a":{"β":"值","a":null}}"#).unwrap();
    let mut b = a.clone();
    b.execution_profile =
        serde_json::from_str(r#"{"a":{"a":null,"β":"值"},"z":[{"a":1,"b":2},false]}"#).unwrap();
    let a = canonicalize_request(a).unwrap();
    assert_eq!(
        a.request_hash(),
        canonicalize_request(b.clone()).unwrap().request_hash()
    );
    assert_eq!(
        a.execution_profile_json(),
        r#"{"a":{"a":null,"β":"值"},"z":[{"a":1,"b":2},false]}"#
    );
    b.execution_profile["z"].as_array_mut().unwrap().reverse();
    assert_ne!(
        a.request_hash(),
        canonicalize_request(b).unwrap().request_hash()
    );
}

#[test]
fn unicode_is_exact_utf8_without_normalization_or_trimming() {
    let mut a = input();
    a.prompt = "中文🦀\n\0é".into();
    let b: String = serde_json::from_str(r#""\u4e2d\u6587\ud83e\udd80\n\u0000\u00e9""#).unwrap();
    let mut other = a.clone();
    other.prompt = b;
    assert_eq!(
        canonicalize_request(a.clone()).unwrap().request_hash(),
        canonicalize_request(other).unwrap().request_hash()
    );
    for prompt in ["中文🦀\n\0e\u{301}", "中文🦀\r\n\0é", " 中文🦀\n\0é"] {
        let mut b = a.clone();
        b.prompt = prompt.into();
        assert_ne!(
            canonicalize_request(a.clone()).unwrap().request_hash(),
            canonicalize_request(b).unwrap().request_hash()
        );
    }
}

#[test]
fn every_variable_input_participates_and_framing_is_unambiguous() {
    let baseline = canonicalize_request(input()).unwrap();
    let mut variants = Vec::new();
    let mut a = input();
    a.agent_id = "b".into();
    variants.push(a);
    let mut a = input();
    a.request_key = "b".into();
    variants.push(a);
    let mut a = input();
    a.prompt = "b".into();
    variants.push(a);
    let mut a = input();
    a.execution_profile = json!({"model":"b"});
    variants.push(a);
    let mut a = input();
    a.workspace_id = "b".into();
    variants.push(a);
    let mut a = input();
    a.canonical_workspace_root = "C:/other".into();
    variants.push(a);
    let mut a = input();
    a.workspace_generation = 2;
    variants.push(a);
    let mut a = input();
    a.provider = ProviderId::new("fake.provider-v1".into()).unwrap();
    variants.push(a);
    let mut a = input();
    a.task_role = AgentTaskRole::Testing;
    variants.push(a);
    let mut a = input();
    a.mode = ExecutionMode::WorkspaceWrite;
    variants.push(a);
    let mut a = input();
    a.parent_execution_id = Some("b".into());
    variants.push(a);
    let mut a = input();
    a.parent_execution_id = Some("".into());
    variants.push(a);
    for variant in variants {
        assert_ne!(
            canonicalize_request(variant).unwrap().request_hash(),
            baseline.request_hash()
        );
    }
    let mut a = input();
    a.agent_id = "a|b".into();
    a.request_key = "c".into();
    let mut b = input();
    b.agent_id = "a".into();
    b.request_key = "b|c".into();
    assert_ne!(
        canonicalize_request(a).unwrap().request_hash(),
        canonicalize_request(b).unwrap().request_hash()
    );
}

#[test]
fn thread_is_runtime_compatibility_but_parent_execution_is_request_identity() {
    let baseline = canonicalize_request(input()).unwrap();
    let mut other_thread = input();
    other_thread.thread_id = Some("provider-thread-b".into());
    assert_eq!(
        canonicalize_request(other_thread).unwrap().request_hash(),
        baseline.request_hash()
    );

    let mut first_parent = input();
    first_parent.parent_execution_id = Some("execution-a".into());
    let mut second_parent = input();
    second_parent.parent_execution_id = Some("execution-b".into());
    assert_ne!(
        canonicalize_request(first_parent).unwrap().request_hash(),
        canonicalize_request(second_parent).unwrap().request_hash()
    );

    let source = include_str!("../execution.rs");
    let current = source
        .split("pub fn canonicalize_request")
        .nth(1)
        .unwrap()
        .split("/// Exact pre-C2")
        .next()
        .unwrap();
    assert!(!current.contains("thread_id"));
}

#[test]
fn opaque_profile_retains_null_types_numbers_and_empty_values() {
    let profiles = [
        json!({}),
        json!({"x":null}),
        json!({"x":false}),
        json!({"x":0}),
        json!({"x":1}),
        json!({"x":1.0}),
        json!({"x":"1"}),
        json!({"x":[]}),
        json!({"x":""}),
    ];
    let hashes: std::collections::HashSet<_> = profiles
        .iter()
        .map(|profile| {
            let mut a = input();
            a.execution_profile = profile.clone();
            canonicalize_request(a).unwrap().request_hash().to_owned()
        })
        .collect();
    assert_eq!(hashes.len(), profiles.len());
    let mut a = input();
    a.execution_profile = Value::Null;
    assert!(canonicalize_request(a).is_err());
    let mut a = input();
    a.workspace_generation = 0;
    assert_eq!(
        canonicalize_request(a).unwrap_err(),
        "workspace_generation must be a positive SQLite integer"
    );
    let mut value = json!({"agent_id":"a","request_key":"k","prompt":"p","execution_profile":{},"workspace_id":"w","canonical_workspace_root":"r","mode":"read_only","new_ignored_field":1});
    assert!(serde_json::from_value::<CreateExecutionInput>(value.clone()).is_err());
    value.as_object_mut().unwrap().remove("new_ignored_field");
    value["provider"] = json!(" other");
    assert!(serde_json::from_value::<CreateExecutionInput>(value).is_err());
}

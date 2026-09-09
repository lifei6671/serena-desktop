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
fn fixed_bytes_and_sha256_vector() {
    let request = canonicalize_request(input()).unwrap();
    assert_eq!(
        std::str::from_utf8(request.bytes()).unwrap(),
        r#"["execution-request-v1","a","k","hello","{}","w","C:/workspace","codex","read_only",null]"#
    );
    assert_eq!(
        request.request_hash(),
        "bf03d1d93d36985767ddb83fba4567fa060b5bde2f8967e29930c1826ee33d82"
    );
    for _ in 0..20 {
        assert_eq!(
            canonicalize_request(input()).unwrap().request_hash(),
            request.request_hash()
        );
    }
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
    a.mode = ExecutionMode::WorkspaceWrite;
    variants.push(a);
    let mut a = input();
    a.thread_id = Some("b".into());
    variants.push(a);
    let mut a = input();
    a.thread_id = Some("".into());
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
    let mut value = json!({"agent_id":"a","request_key":"k","prompt":"p","execution_profile":{},"workspace_id":"w","canonical_workspace_root":"r","mode":"read_only","new_ignored_field":1});
    assert!(serde_json::from_value::<CreateExecutionInput>(value.clone()).is_err());
    value.as_object_mut().unwrap().remove("new_ignored_field");
    value["provider"] = json!("other");
    assert!(serde_json::from_value::<CreateExecutionInput>(value).is_err());
}

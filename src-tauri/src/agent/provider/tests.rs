use super::*;
use serde_json::{Value, json};

#[test]
fn provider_id_accepts_opaque_values_and_serializes_as_a_json_string() {
    for raw in ["codex", "provider.v2/@_:+~", "提供者-β"] {
        let id = ProviderId::new(raw.to_owned()).unwrap();
        assert_eq!(id.as_str(), raw);

        let wire = serde_json::to_value(&id).unwrap();
        assert_eq!(wire, Value::String(raw.to_owned()));
        assert_eq!(serde_json::from_value::<ProviderId>(wire).unwrap(), id);
    }
}

#[test]
fn provider_id_rejects_empty_whitespace_and_control_characters_everywhere() {
    for invalid in [
        "",
        "with space",
        "with\ttab",
        "with\nnewline",
        "with\u{00a0}nonbreaking-space",
        "with\u{2003}em-space",
        "with\u{0007}control",
        "with\u{007f}delete",
    ] {
        assert!(ProviderId::new(invalid.to_owned()).is_err(), "{invalid:?}");
        assert!(
            serde_json::from_value::<ProviderId>(Value::String(invalid.to_owned())).is_err(),
            "{invalid:?}"
        );
    }

    assert!(serde_json::from_value::<ProviderId>(json!({ "value": "codex" })).is_err());
}

#[test]
fn provider_contexts_have_only_the_frozen_execution_id_field() {
    let execution = ProviderExecutionContext {
        execution_id: "execution-1".into(),
    };
    let cancel = ProviderCancelContext {
        execution_id: "execution-1".into(),
    };
    assert_eq!(
        serde_json::to_value(execution).unwrap(),
        json!({ "executionId": "execution-1" })
    );
    assert_eq!(
        serde_json::to_value(cancel).unwrap(),
        json!({ "executionId": "execution-1" })
    );

    for forged in [
        "workspaceId",
        "canonicalRoot",
        "threadId",
        "turnId",
        "job",
        "historyMode",
        "runtimeId",
        "runtimeInstanceId",
    ] {
        let mut value = json!({ "executionId": "execution-1" });
        value[forged] = json!("forged");
        assert!(
            serde_json::from_value::<ProviderExecutionContext>(value.clone()).is_err(),
            "execution context accepted {forged}"
        );
        assert!(
            serde_json::from_value::<ProviderCancelContext>(value).is_err(),
            "cancel context accepted {forged}"
        );
    }

    assert!(
        serde_json::from_value::<ProviderExecutionContext>(
            json!({ "executionId": "execution-1", "unknown": true })
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ProviderCancelContext>(
            json!({ "executionId": "execution-1", "unknown": true })
        )
        .is_err()
    );
}

#[test]
fn startup_context_accepts_only_an_empty_object() {
    assert_eq!(
        serde_json::to_value(ProviderStartupContext {}).unwrap(),
        json!({})
    );
    assert!(serde_json::from_value::<ProviderStartupContext>(json!({})).is_ok());
    assert!(
        serde_json::from_value::<ProviderStartupContext>(json!({ "executionId": "forged" }))
            .is_err()
    );
}

#[test]
fn provider_error_code_wire_values_are_exact() {
    for (code, wire) in [
        (
            ProviderErrorCode::AgentProviderNotFound,
            "AGENT_PROVIDER_NOT_FOUND",
        ),
        (
            ProviderErrorCode::AgentProviderUnavailable,
            "AGENT_PROVIDER_UNAVAILABLE",
        ),
        (
            ProviderErrorCode::AgentProviderCapabilityUnsupported,
            "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED",
        ),
        (
            ProviderErrorCode::AgentProviderContractError,
            "AGENT_PROVIDER_CONTRACT_ERROR",
        ),
        (
            ProviderErrorCode::AgentProviderOperationFailed,
            "AGENT_PROVIDER_OPERATION_FAILED",
        ),
    ] {
        assert_eq!(serde_json::to_value(code).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<ProviderErrorCode>(json!(wire)).unwrap(),
            code
        );
    }
}

#[test]
fn provider_error_contains_only_code_and_rejects_private_details() {
    let error = ProviderError {
        code: ProviderErrorCode::AgentProviderOperationFailed,
    };
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({ "code": "AGENT_PROVIDER_OPERATION_FAILED" })
    );

    for forged in [
        "message",
        "rawMessage",
        "rawProviderMessage",
        "providerDetails",
        "command",
        "stdout",
        "stderr",
        "runtime",
        "runtimeId",
        "thread",
        "threadId",
        "turn",
        "turnId",
        "job",
    ] {
        let mut value = json!({ "code": "AGENT_PROVIDER_OPERATION_FAILED" });
        value[forged] = json!("forged");
        assert!(
            serde_json::from_value::<ProviderError>(value).is_err(),
            "provider error accepted {forged}"
        );
    }
}

#[test]
fn provider_outcome_and_result_completeness_wire_values_are_exact() {
    for (outcome, wire) in [
        (ProviderOutcome::Completed, "completed"),
        (ProviderOutcome::Failed, "failed"),
        (ProviderOutcome::Cancelled, "cancelled"),
        (ProviderOutcome::Interrupted, "interrupted"),
    ] {
        assert_eq!(serde_json::to_value(outcome).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<ProviderOutcome>(json!(wire)).unwrap(),
            outcome
        );
    }

    for (completeness, wire) in [
        (ProviderResultCompleteness::Unknown, "unknown"),
        (ProviderResultCompleteness::Partial, "partial"),
        (ProviderResultCompleteness::Complete, "complete"),
    ] {
        assert_eq!(serde_json::to_value(completeness).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<ProviderResultCompleteness>(json!(wire)).unwrap(),
            completeness
        );
    }
}

#[test]
fn provider_run_result_round_trips_with_exact_field_names() {
    let result = ProviderRunResult {
        execution_id: "execution-1".into(),
        outcome: ProviderOutcome::Completed,
        result: Some(json!({ "answer": 42 })),
        result_completeness: ProviderResultCompleteness::Complete,
        diagnostic_code: Some("PROVIDER_DIAGNOSTIC".into()),
    };
    let wire = serde_json::to_value(&result).unwrap();
    assert_eq!(
        wire,
        json!({
            "executionId": "execution-1",
            "outcome": "completed",
            "result": { "answer": 42 },
            "resultCompleteness": "complete",
            "diagnosticCode": "PROVIDER_DIAGNOSTIC"
        })
    );
    assert_eq!(
        serde_json::from_value::<ProviderRunResult>(wire).unwrap(),
        result
    );
}

#[test]
fn provider_run_result_rejects_forged_safety_and_private_identity_fields() {
    for forged in [
        "safe",
        "safeToReleaseWorkspace",
        "releaseEvidence",
        "jobEmpty",
        "runtimeTerminated",
        "cleanupComplete",
        "threadId",
        "turnId",
        "job",
        "historyMode",
        "runtimeId",
        "runtimeInstanceId",
        "runtimeIdentity",
    ] {
        let mut value = json!({
            "executionId": "execution-1",
            "outcome": "completed",
            "result": null,
            "resultCompleteness": "unknown",
            "diagnosticCode": null
        });
        value[forged] = json!(true);
        assert!(
            serde_json::from_value::<ProviderRunResult>(value).is_err(),
            "provider run result accepted {forged}"
        );
    }
}

#[test]
fn production_provider_domain_has_no_private_or_safety_identity_members() {
    let source = include_str!("mod.rs");
    for forbidden in [
        "safe",
        "safe_to_release_workspace",
        "release_evidence",
        "job_empty",
        "runtime_terminated",
        "cleanup_complete",
        "workspace_id",
        "canonical_root",
        "canonical_workspace_root",
        "workspace_root",
        "claim",
        "claim_release_authorized",
        "runtime",
        "thread",
        "thread_id",
        "turn",
        "turn_id",
        "job",
        "history_mode",
        "runtime_id",
        "runtime_instance_id",
        "runtime_identity",
    ] {
        assert!(
            !source.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with(&format!("{forbidden}:"))
                    || line.starts_with(&format!("pub {forbidden}:"))
                    || line.starts_with(&format!("pub(crate) {forbidden}:"))
                    || line.starts_with(&format!("pub(super) {forbidden}:"))
            }),
            "production domain declares forbidden member {forbidden}"
        );
    }
}

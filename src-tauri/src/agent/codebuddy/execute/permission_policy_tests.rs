//! 本地 policy 的完整原生 Provider 路径：真实 pipe、Store、Activity 与释放证据。
use super::*;

/// cargo 在 workspace_write 被允许、read_only 被拒绝；危险命令在 workspace_write 仍拒绝。
#[tokio::test]
async fn native_policy_allow_and_readonly_reject_preserve_terminal_authority() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for (execution_mode, peer_mode, allowed) in [
        ("write", "permission-allow", true),
        ("read", "permission-policy-readonly", false),
        ("write", "permission-dangerous", false),
    ] {
        // 复用 setup 创建正确 Execution mode，随后只修改隔离 peer 的控制模式。
        let (control, _workspace, store, provider) = setup(&binary, execution_mode).await;
        std::fs::write(control.path().join("mode"), peer_mode).unwrap();
        std::fs::write(
            control.path().join("permission-mode.json"),
            json!({"catalog":"modes"}).to_string(),
        )
        .unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(25),
            provider.execute(
                ProviderExecutionContext {
                    execution_id: "e".into(),
                },
                Arc::new(Sink(control.path().into())),
                Arc::new(
                    crate::agent::telemetry_projector::ExecutionTelemetryProjector::new(
                        store.clone(),
                        "e".into(),
                    ),
                ),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            result.outcome,
            if allowed {
                ProviderOutcome::Completed
            } else {
                ProviderOutcome::Cancelled
            }
        );
        let response: Value = serde_json::from_str(
            &std::fs::read_to_string(control.path().join("permission-response.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            response["result"]["outcome"]["optionId"],
            if allowed {
                "allow"
            } else {
                "advertised-deny-id"
            }
        );
        assert_eq!(
            request_methods(control.path()),
            vec![
                "initialize",
                "session/new",
                "session/prompt",
                "permission-response"
            ]
        );
        assert!(!control.path().join("set-mode.json").exists());
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(
            row.provider_terminal_status.as_deref(),
            Some(if allowed { "completed" } else { "cancelled" })
        );
        assert_eq!(row.status, if allowed { "completed" } else { "cancelled" });
        assert_eq!(row.release_evidence_state, "complete");
        assert_eq!(
            row.release_evidence_kind.as_deref(),
            Some("runtime_terminated")
        );
        let runtime = store
            .runtime(row.runtime_instance_id.clone().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(runtime.state, "terminated");
        assert_eq!(runtime.termination_evidence_state, "complete");
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        let history = store
            .execution_activity_history("e".into(), None, Some(100))
            .await
            .unwrap();
        assert_eq!(
            history
                .events
                .iter()
                .any(|activity| activity.summary_code.as_deref()
                    == Some("provider.permission_denied")),
            !allowed
        );
        if allowed {
            assert!(
                history
                    .events
                    .iter()
                    .any(|activity| activity.summary_code.as_deref() == Some("tool.command"))
            );
            assert_ne!(
                row.error_code.as_deref(),
                Some("CODEBUDDY_PERMISSION_DENIED")
            );
            assert_ne!(
                row.activity_summary_code.as_deref(),
                Some("provider.permission_denied")
            );
        } else {
            // RejectOnce 仍使用既有安全诊断，不改变原 owner 的终态与释放职责。
            assert_eq!(
                row.error_code.as_deref(),
                Some("CODEBUDDY_PERMISSION_DENIED")
            );
        }
    }
}

//! 默认保留 Provider 当前权限模式的原生契约；不依赖真实 CodeBuddy 或用户配置。
use super::*;
use std::time::Duration;

/// 复用 Fresh/Continue 的完整生产事务，权限控制只在 source 已完成后启用。
async fn permission_setup(
    binary: &Path,
    continued: bool,
    catalog: &str,
    permission_request: bool,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    StateStore,
    Arc<CodeBuddyProvider>,
) {
    let (control, workspace, store, provider) = if continued {
        setup_continuation(
            binary,
            if permission_request {
                "continue-permission"
            } else {
                "continue-success"
            },
        )
        .await
    } else {
        let (control, workspace, store, provider) = setup(
            binary,
            if permission_request {
                "permission-read"
            } else {
                "read"
            },
        )
        .await;
        (control, workspace, store, Arc::new(provider))
    };
    std::fs::write(
        control.path().join("permission-mode.json"),
        json!({"catalog":catalog}).to_string(),
    )
    .unwrap();
    (control, workspace, store, provider)
}

/// acceptance 当场禁止隐式模式配置，再委托已有 durable Prompt/Claim 验证。
struct ModeAcceptance {
    control: PathBuf,
    continued: bool,
}
impl ProviderAcceptanceSink for ModeAcceptance {
    /// 默认执行无需模式 ACK，物理 Prompt 在已有 sink 中仍严格禁止。
    fn accepted(&self) {
        assert!(!self.control.join("set-mode.json").exists());
        if self.continued {
            ContinuationSink(self.control.clone()).accepted();
        } else {
            Sink(self.control.clone()).accepted();
        }
    }
}

/// 所有模式用同一个 public execute 入口，保留真实请求、SQLite 和 Runtime evidence。
async fn execute_permission_case(
    provider: Arc<CodeBuddyProvider>,
    store: StateStore,
    control: PathBuf,
    continued: bool,
) -> Result<ProviderRunResult, crate::agent::provider::port::ProviderExecutionFailure> {
    let execution_id: String = if continued { "c" } else { "e" }.into();
    provider
        .execute(
            ProviderExecutionContext {
                execution_id: execution_id.clone(),
            },
            Arc::new(ModeAcceptance { control, continued }),
            Arc::new(
                crate::agent::telemetry_projector::ExecutionTelemetryProjector::new(
                    store,
                    execution_id,
                ),
            ),
        )
        .await
}

/// 原生请求与公共 profile 都保留模型/推理语义，权限模式不进入保存的 profile。
async fn assert_profile_and_methods(control: &Path, store: &StateStore, continued: bool) {
    let row = store
        .execution(if continued { "c" } else { "e" }.into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(row.effective_execution_profile_json.as_deref().unwrap())
            .unwrap(),
        json!({"model":"model-a","reasoning":"medium"})
    );
    assert_eq!(
        serde_json::from_str::<Value>(&row.execution_profile_json).unwrap(),
        json!({})
    );
    assert_eq!(
        request_methods(control),
        vec![
            "initialize",
            if continued {
                "session/load"
            } else {
                "session/new"
            },
            "session/prompt",
        ]
    );
    assert!(!control.join("set-mode.json").exists());
    assert!(!control.join("forbidden-session-new").exists());
}

/// advertise auto 也不触发 set_mode/ACK；Fresh/Continue 直接 prompt 且不改 source。
#[tokio::test]
async fn native_advertised_auto_keeps_current_mode_for_fresh_and_continue() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for continued in [false, true] {
        for catalog in ["modes", "config"] {
            let (control, _workspace, store, provider) =
                permission_setup(&binary, continued, catalog, false).await;
            let source = if continued {
                Some(store.read_codebuddy_state("e".into()).await.unwrap())
            } else {
                None
            };
            // peer 收到任何隐式 set_mode 会立即失败；默认路径必须直接进入 prompt。
            let result = tokio::time::timeout(
                Duration::from_secs(15),
                execute_permission_case(
                    provider.clone(),
                    store.clone(),
                    control.path().into(),
                    continued,
                ),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(result.outcome, ProviderOutcome::Completed);
            assert_profile_and_methods(control.path(), &store, continued).await;
            assert_eq!(provider.admission_diagnostic(), None);
            if let Some(source) = source {
                let unchanged = store.read_codebuddy_state("e".into()).await.unwrap();
                assert_eq!(unchanged.session_id, source.session_id);
                assert_eq!(unchanged.provider_request_id, source.provider_request_id);
                assert_eq!(unchanged.runtime_instance_id, source.runtime_instance_id);
                assert_eq!(
                    unchanged.conversation_request_id,
                    source.conversation_request_id
                );
                let child = store.read_codebuddy_state("c".into()).await.unwrap();
                assert_eq!(child.session_id, source.session_id);
                assert!(child.provider_request_id.is_none());
                assert_ne!(child.runtime_instance_id, source.runtime_instance_id);
            }
        }
    }
}

/// 不 advertise auto 的 Session 继续原模式，缺失能力不能使 Provider unavailable。
#[tokio::test]
async fn native_absent_auto_keeps_original_mode_and_provider_admission() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for continued in [false, true] {
        let (control, _workspace, store, provider) =
            permission_setup(&binary, continued, "no-auto", false).await;
        let result = execute_permission_case(
            provider.clone(),
            store.clone(),
            control.path().into(),
            continued,
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, ProviderOutcome::Completed);
        assert!(!control.path().join("set-mode.json").exists());
        assert_profile_and_methods(control.path(), &store, continued).await;
        assert_eq!(provider.admission_diagnostic(), None);
    }
}

/// 保留当前 mode 后的未授权工具仍拒绝；Fresh/Continue 使用 exact Session 的 RejectOnce。
#[tokio::test]
async fn native_current_mode_permission_still_rejects_once() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for continued in [false, true] {
        for catalog in ["modes", "config"] {
            let (control, _workspace, store, provider) =
                permission_setup(&binary, continued, catalog, true).await;
            let result =
                execute_permission_case(provider, store.clone(), control.path().into(), continued)
                    .await
                    .unwrap();
            assert_eq!(result.outcome, ProviderOutcome::Cancelled);
            let response: Value = serde_json::from_str(
                &std::fs::read_to_string(control.path().join("permission-response.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                response,
                json!({"jsonrpc":"2.0","id":0,"result":{"outcome":{"outcome":"selected","optionId":"advertised-deny-id"}}})
            );
            assert_eq!(
                request_methods(control.path()),
                vec![
                    "initialize",
                    if continued {
                        "session/load"
                    } else {
                        "session/new"
                    },
                    "session/prompt",
                    "permission-response"
                ]
            );
            let row = store
                .execution(if continued { "c" } else { "e" }.into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                row.error_code.as_deref(),
                Some("CODEBUDDY_PERMISSION_DENIED")
            );
            assert_eq!(row.release_evidence_state, "complete");
            assert_eq!(
                serde_json::from_str::<Value>(
                    row.effective_execution_profile_json.as_deref().unwrap()
                )
                .unwrap(),
                json!({"model":"model-a","reasoning":"medium"})
            );
            assert!(!control.path().join("cancel.json").exists());
        }
    }
}

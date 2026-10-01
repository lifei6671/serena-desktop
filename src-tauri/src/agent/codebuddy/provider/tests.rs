use super::*;
use crate::agent::provider::port::{AgentEventSink, ProviderAcceptanceSink};
use std::path::PathBuf;
#[cfg(windows)]
use std::{
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    time::{Duration, Instant},
};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    },
};

/// lifecycle fail-closed 测试的 acceptance sentinel。
struct NoopAcceptance;

impl ProviderAcceptanceSink for NoopAcceptance {
    /// skeleton 测试不应接受执行，此方法仅满足 trait 输入。
    fn accepted(&self) {
        panic!("CodeBuddy skeleton must not accept execution")
    }
}

/// lifecycle fail-closed 测试的无状态 telemetry sink。
struct NoopTelemetry;

impl AgentEventSink for NoopTelemetry {}

/// missing CLI 仍注册 descriptor，但 Registry execution lookup 保持 unavailable。
#[tokio::test]
async fn missing_cli_registers_unavailable_skeleton() {
    let (_directory, store) = authority().await;
    let mut registry = ProviderRegistry::new();
    register_codebuddy_provider_with_discovery(
        &mut registry,
        store.clone(),
        "test-host".into(),
        Err(DiscoveryError::not_found(true)),
    )
    .unwrap();
    let id = ProviderId::new("codebuddy".into()).unwrap();
    let provider = registry.get_registered(&id).unwrap();
    assert_eq!(
        provider.descriptor(),
        ProviderDescriptor {
            id: id.clone(),
            display_name: "CodeBuddy".into(),
            version: None,
            protocol: Some("ACP v1".into()),
        }
    );
    assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Unavailable);
    match registry.get(&id) {
        Err(error) => assert_eq!(error.code, ProviderErrorCode::AgentProviderUnavailable),
        Ok(_) => panic!("unavailable CodeBuddy resolved for execution"),
    }
    assert_eq!(
        provider.capabilities(),
        ProviderCapabilities {
            can_execute: cfg!(any(windows, target_os = "macos")),
            can_continue: cfg!(any(windows, target_os = "macos")),
            can_cancel: cfg!(any(windows, target_os = "macos")),
            can_recover: cfg!(any(windows, target_os = "macos")),
            activity: cfg!(any(windows, target_os = "macos")),
            token_usage: false,
        }
    );
}

/// found CLI 与 enabled 在已验证平台允许 Execute，能力不依赖 CLI presence。
#[tokio::test]
async fn found_cli_registers_available_platform_capabilities() {
    let (_directory, store) = authority().await;
    let mut registry = ProviderRegistry::new();
    register_codebuddy_provider_with_discovery(
        &mut registry,
        store.clone(),
        "test-host".into(),
        Ok(DiscoveryResult::direct_for_test(
            "C:/resolved/codebuddy.exe",
        )),
    )
    .unwrap();
    let id = ProviderId::new("codebuddy".into()).unwrap();
    assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Available);
    let registered = registry.get_registered(&id).unwrap();
    assert!(Arc::ptr_eq(&registered, &registry.get(&id).unwrap()));
    assert_eq!(
        registered.capabilities(),
        ProviderCapabilities {
            can_execute: cfg!(any(windows, target_os = "macos")),
            can_continue: cfg!(any(windows, target_os = "macos")),
            can_cancel: cfg!(any(windows, target_os = "macos")),
            can_recover: cfg!(any(windows, target_os = "macos")),
            activity: cfg!(any(windows, target_os = "macos")),
            token_usage: false,
        }
    );
    // Registry get 只检查 health；新执行还必须通过已有 capability admission。
    use crate::agent::provider::control::{ProviderAdmissionCapability, ProviderAdmissionPolicy};
    let policy = ProviderAdmissionPolicy::new(Default::default());
    policy.set_enabled_for_test("codebuddy", true);
    let admission = policy.admit(&registry, &id, ProviderAdmissionCapability::Execute);
    assert_eq!(admission.is_ok(), cfg!(any(windows, target_os = "macos")));
    if let Err(error) = admission {
        assert_eq!(
            error.code,
            ProviderErrorCode::AgentProviderCapabilityUnsupported
        );
    }
    let continuation = policy.admit(&registry, &id, ProviderAdmissionCapability::Continue);
    assert_eq!(
        continuation.is_ok(),
        cfg!(any(windows, target_os = "macos"))
    );
}

/// 缺 source private row/sessionId 时 Provider validation 必须 Ineligible，不推造身份。
#[tokio::test]
async fn continuation_requires_exact_source_private_identity() {
    let (directory, store) = authority().await;
    let input: crate::agent::execution::CreateExecutionInput =
        serde_json::from_value(serde_json::json!({
            "agent_id":"a","request_key":"k","prompt":"source","execution_profile":{},
            "workspace_id":"w","canonical_workspace_root":directory.path(),
            "workspace_generation":1,"provider":"codebuddy","mode":"workspace_write"
        }))
        .unwrap();
    store
        .create_execution(
            "source".into(),
            crate::agent::execution::canonicalize_request(input).unwrap(),
            crate::agent::coordinator::now(),
        )
        .await
        .unwrap();
    let provider = CodeBuddyProvider::from_discovery(
        store,
        "test-host".into(),
        Ok(DiscoveryResult::direct_for_test(
            "C:/resolved/codebuddy.exe",
        )),
    );
    assert_eq!(
        provider
            .validate_continuation(ProviderContinuationContext {
                source_execution_id: "source".into(),
            })
            .await
            .unwrap(),
        ProviderContinuationDecision::Ineligible
    );
}

/// resolved LaunchSpec 与 Release descriptor 分离，descriptor 不获得本机绝对路径。
#[tokio::test]
async fn resolved_launch_spec_is_separate_from_default_descriptor() {
    let (_directory, store) = authority().await;
    let discovery = DiscoveryResult::direct_for_test("C:/resolved/node.exe");
    let provider =
        CodeBuddyProvider::from_discovery(store.clone(), "test-host".into(), Ok(discovery));
    assert_eq!(DEFAULT_LAUNCH_DESCRIPTOR.command, "codebuddy");
    assert_eq!(DEFAULT_LAUNCH_DESCRIPTOR.args, ["--acp"]);
    assert_eq!(
        provider.resolved_launch_spec().unwrap().executable,
        PathBuf::from("C:/resolved/node.exe")
    );
    assert_eq!(provider.descriptor().version, None);
    assert!(provider.discovery_provenance().is_some());
    assert!(provider.discovery_error().is_none());
}

/// base/package version 与路径中的 hash 不能冒充公开版本；只展示解析后的 product_version。
#[tokio::test]
async fn descriptor_uses_only_product_version_metadata() {
    let (_directory, store) = authority().await;
    let mut discovery =
        DiscoveryResult::direct_for_test("C:/resolved/build-deadbeef/codebuddy.exe");
    discovery.metadata.base_version = Some("1.106.1".into());
    discovery.metadata.package_version = Some("0.0.0-deadbeef".into());
    discovery.metadata.status = super::super::discovery::MetadataStatus::Parsed;
    let provider =
        CodeBuddyProvider::from_discovery(store.clone(), "test-host".into(), Ok(discovery.clone()));
    assert_eq!(
        provider.descriptor(),
        ProviderDescriptor {
            id: ProviderId::new("codebuddy".into()).unwrap(),
            display_name: "CodeBuddy".into(),
            version: None,
            protocol: Some("ACP v1".into()),
        }
    );
    discovery.metadata.product_version = Some("2.158.0".into());
    let provider =
        CodeBuddyProvider::from_discovery(store.clone(), "test-host".into(), Ok(discovery));
    assert_eq!(
        provider.descriptor(),
        ProviderDescriptor {
            id: ProviderId::new("codebuddy".into()).unwrap(),
            display_name: "CodeBuddy".into(),
            version: Some("2.158.0".into()),
            protocol: Some("ACP v1".into()),
        }
    );
}

/// 等待 catalog fake 写出 PID，并持有可观察进程 handle。
#[cfg(windows)]
async fn wait_catalog_process(path: &std::path::Path) -> OwnedHandle {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(Instant::now() < deadline, "{}", path.display());
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let pid = std::fs::read_to_string(path)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    // SAFETY: PID 来自当前测试启动的隔离 fake，OwnedHandle 接管返回 handle。
    let raw = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    assert!(!raw.is_null());
    unsafe { OwnedHandle::from_raw_handle(raw) }
}

/// Provider catalog 使用正式 launcher/ACP Runtime，成功和错误都必须终止进程且不写 Execution/Claim。
#[cfg(windows)]
#[tokio::test]
async fn configuration_catalog_managed_job_converges_without_product_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().join("base.exe");
    let output = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "codebuddy_fresh_child"])
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/codebuddy_fresh_child.rs"),
        )
        .arg("-o")
        .arg(&base)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    for error in [false, true] {
        let workspace = directory.path().join(format!("workspace-{error}"));
        std::fs::create_dir_all(&workspace).unwrap();
        let executable = workspace.join(if error {
            "cb7-fresh-gated-new-error.exe"
        } else {
            "cb7-fresh-gated.exe"
        });
        std::fs::copy(&base, &executable).unwrap();
        std::fs::write(
            workspace.join("new.json"),
            serde_json::json!({
                "sessionId":"catalog-session",
                "configOptions":[
                    {"id":"model","name":"Model","category":"model","type":"select","currentValue":"model-a","options":[{"value":"model-a","name":"Model A"}]},
                    {"id":"thought_level","name":"Reasoning","category":"thought_level","type":"select","currentValue":"high","options":[{"value":"high","name":"High"}]}
                ],
                "models":{"currentModelId":"model-a","availableModels":[{"modelId":"model-a","name":"Model A","_meta":{"supportsReasoning":true}}]}
            })
            .to_string(),
        )
        .unwrap();
        let data = directory.path().join(format!("data-{error}"));
        let store = crate::agent::store::StateStore::open(data.clone())
            .await
            .unwrap();
        let mut discovery = DiscoveryResult::direct_for_test(executable);
        discovery.launch_spec.path_projection = vec![workspace.clone()];
        let provider = Arc::new(CodeBuddyProvider::from_discovery(
            store,
            "catalog-test-host".into(),
            Ok(discovery),
        ));
        let task = tokio::spawn({
            let provider = provider.clone();
            let root = crate::config::canonicalize_workspace_root(&workspace)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            async move {
                provider
                    .configuration_catalog(ProviderConfigurationCatalogContext { cwd: root })
                    .await
            }
        });
        let pid_path = workspace.join("peer-pid.txt");
        let deadline = Instant::now() + Duration::from_secs(15);
        while !pid_path.exists() {
            if task.is_finished() {
                panic!(
                    "catalog exited before fake peer start: {:?}",
                    task.await.unwrap()
                );
            }
            assert!(Instant::now() < deadline, "{}", pid_path.display());
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let process = wait_catalog_process(&pid_path).await;
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle(), 0) },
            WAIT_TIMEOUT
        );
        std::fs::write(workspace.join("release-initialize"), "1").unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let wire = std::fs::read_to_string(workspace.join("wire.jsonl")).unwrap_or_default();
            if wire.lines().count() >= 2 {
                break;
            }
            assert!(Instant::now() < deadline, "session/new was not observed");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        std::fs::write(workspace.join("release-new"), "1").unwrap();
        let result = task.await.unwrap();
        if error {
            assert_eq!(
                result.unwrap_err().code,
                ProviderErrorCode::AgentProviderOperationFailed
            );
        } else {
            let catalog = result.unwrap();
            assert_eq!(catalog.models[0].id, "model-a");
            assert_eq!(catalog.current_reasoning.as_deref(), Some("high"));
        }
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) },
            WAIT_OBJECT_0
        );
        let connection = rusqlite::Connection::open(data.join("agent-state.db")).unwrap();
        for table in ["executions", "workspace_claims", "runtime_instances"] {
            let count: i64 = connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table}");
        }
    }
}

/// 绕过 admission 也不能 execute/cancel；startup 可独立恢复空的历史集合。
#[tokio::test]
async fn lifecycle_methods_never_start_unimplemented_behavior() {
    let (_directory, store) = authority().await;
    let provider = CodeBuddyProvider::from_discovery(
        store.clone(),
        "test-host".into(),
        Ok(DiscoveryResult::direct_for_test(
            "C:/resolved/codebuddy.exe",
        )),
    );
    let execute = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "execution".into(),
            },
            Arc::new(NoopAcceptance),
            Arc::new(NoopTelemetry),
        )
        .await;
    assert_eq!(
        execute,
        Err(ProviderExecutionFailure::State(
            if cfg!(any(windows, target_os = "macos")) {
                "EXECUTION_NOT_FOUND"
            } else {
                "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED"
            }
            .into()
        ))
    );
    assert_eq!(
        provider
            .cancel(ProviderCancelContext {
                execution_id: "execution".into(),
            })
            .await
            .unwrap_err()
            .code,
        if cfg!(any(windows, target_os = "macos")) {
            ProviderErrorCode::AgentProviderOperationFailed
        } else {
            ProviderErrorCode::AgentProviderCapabilityUnsupported
        }
    );
    assert!(
        provider
            .startup_reconcile(ProviderStartupContext {})
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

/// 测试 Store 和临时目录具有同一测试生命周期。
async fn authority() -> (tempfile::TempDir, crate::agent::store::StateStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::agent::store::StateStore::open(directory.path().into())
        .await
        .unwrap();
    (directory, store)
}

/// CB8：历史 control 与 CLI health 无关；直调错误 provider 的 Execution 必须拒绝。
#[cfg(any(windows, target_os = "macos"))]
#[tokio::test]
async fn cancel_unavailable_registered_history_and_wrong_provider() {
    use crate::agent::execution::{CreateExecutionInput, canonicalize_request};
    let (_dir, store) = authority().await;
    let mut registry = ProviderRegistry::new();
    register_codebuddy_provider_with_discovery(
        &mut registry,
        store.clone(),
        "host".into(),
        Err(DiscoveryError::not_found(false)),
    )
    .unwrap();
    let provider = registry
        .get_registered(&ProviderId::new("codebuddy".into()).unwrap())
        .unwrap();
    for id in ["codebuddy", "codex"] {
        let input: CreateExecutionInput = serde_json::from_value(serde_json::json!({"agent_id":id,"request_key":id,"prompt":"test","execution_profile":{},"workspace_id":id,"canonical_workspace_root":id,"mode":"read_only","provider":id})).unwrap();
        store
            .create_execution(id.into(), canonicalize_request(input).unwrap(), 1)
            .await
            .unwrap();
    }
    assert!(
        provider
            .cancel(ProviderCancelContext {
                execution_id: "codex".into()
            })
            .await
            .is_err()
    );
    assert_eq!(
        store
            .execution("codex".into())
            .await
            .unwrap()
            .unwrap()
            .status,
        "dispatch_pending"
    );
    store
        .reserve_runtime_attempt("codebuddy".into(), "original".into(), 2)
        .await
        .unwrap();
    provider
        .cancel(ProviderCancelContext {
            execution_id: "codebuddy".into(),
        })
        .await
        .unwrap();
    let row = store.execution("codebuddy".into()).await.unwrap().unwrap();
    assert!(row.interrupt_requested_at.is_some());
    assert_eq!(row.status, "dispatch_pending");
    assert!(
        store
            .workspace_claim("codebuddy".into())
            .await
            .unwrap()
            .is_some()
    );
    provider
        .cancel(ProviderCancelContext {
            execution_id: "codebuddy".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .execution("codebuddy".into())
            .await
            .unwrap()
            .unwrap()
            .revision,
        row.revision
    );
}

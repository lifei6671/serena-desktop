//! public Provider native vertical slice；真实 Job-at-creation、pipe 与 SQLite。
use super::*;
use crate::agent::codebuddy::{discovery::DiscoveryResult, provider::CodeBuddyProvider};
use crate::agent::{
    execution::{CreateExecutionInput, canonicalize_request},
    provider::{ProviderExecutionContext, ProviderStartupContext, port::AgentProvider},
};
use serde_json::{Value, json};
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
};

struct Sink(PathBuf);
impl ProviderAcceptanceSink for Sink {
    /// acceptance 必须在 private Sent 后、generic Dispatching 与物理 Prompt 前。
    fn accepted(&self) {
        let db = rusqlite::Connection::open(self.0.join("agent-state.db")).unwrap();
        let (dispatch, private): (String,String) = db.query_row("SELECT dispatch_state,prompt_state FROM executions JOIN codebuddy_execution_state ON executions.id=codebuddy_execution_state.execution_id", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(dispatch, "not_dispatched");
        assert_eq!(private, "sent");
        assert!(!self.0.join("prompt.json").exists());
        std::fs::write(self.0.join("accepted"), "").unwrap();
    }
}
struct SlowTelemetry;
impl AgentEventSink for SlowTelemetry {
    /// 永不完成的 activity consumer 不能阻塞 terminal/finalization。
    fn publish<'a>(
        &'a self,
        event: crate::agent::provider::telemetry::AgentTelemetryEvent,
    ) -> crate::agent::provider::port::ProviderFuture<'a, ()> {
        let safe = format!("{event:?}");
        assert!(!safe.contains("private command"));
        Box::pin(std::future::pending())
    }
}

/// 复用当前 Cargo 已构建依赖；fake child 本身独立 native executable。
fn build(dir: &Path) -> PathBuf {
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let mut command = std::process::Command::new("rustc");
    command
        .args(["--edition=2024", "--crate-name", "codebuddy_execute_child"])
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codebuddy_execute_child.rs"),
        )
        .arg("-L")
        .arg(format!("dependency={}", deps.display()));
    for name in ["rusqlite", "serde_json"] {
        let library = std::fs::read_dir(&deps)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("lib{name}-"))
                    && e.path().extension().is_some_and(|v| v == "rlib")
            })
            .max_by_key(|e| e.metadata().unwrap().modified().unwrap())
            .expect("built dependency")
            .path();
        command
            .arg("--extern")
            .arg(format!("{name}={}", library.display()));
    }
    let binary = dir.join("peer.exe");
    let output = command
        .arg("-o")
        .arg(&binary)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

/// Workspace 与 Store/control 分离，所有真实文件 delta 可完整核对。
async fn setup(
    binary: &Path,
    mode: &str,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    StateStore,
    CodeBuddyProvider,
) {
    let control = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let store = StateStore::open(control.path().into()).await.unwrap();
    let root = crate::config::canonicalize_workspace_root(workspace.path()).unwrap();
    let input: CreateExecutionInput = serde_json::from_value(json!({"agent_id":"a","request_key":"k","prompt":"fixed fake input","execution_profile":{},"workspace_id":"w","canonical_workspace_root":root,"mode":if mode=="write" {"workspace_write"} else {"read_only"},"provider":"codebuddy"})).unwrap();
    store
        .create_execution("e".into(), canonicalize_request(input).unwrap(), now())
        .await
        .unwrap();
    let db = rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap();
    db.execute_batch("CREATE TABLE cb7_trace(status TEXT,dispatch TEXT,claims INTEGER,runtime_state TEXT,evidence TEXT); CREATE TRIGGER cb7_trace AFTER UPDATE OF status,dispatch_state ON executions BEGIN INSERT INTO cb7_trace SELECT NEW.status,NEW.dispatch_state,(SELECT count(*) FROM workspace_claims),state,termination_evidence_state FROM runtime_instances WHERE id=NEW.runtime_instance_id; END;").unwrap();
    std::fs::write(control.path().join("mode"), mode).unwrap();
    let peer = control.path().join("peer.exe");
    std::fs::copy(binary, &peer).unwrap();
    let discovery = DiscoveryResult::direct_for_test(peer);
    let provider = CodeBuddyProvider::from_discovery(store.clone(), "host".into(), Ok(discovery));
    (control, workspace, store, provider)
}

#[tokio::test]
/// 真正 public execute 完成 isolated write/read-only；slow Activity 不改变结果。
async fn native_public_execute_write_and_readonly_atomic_release() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for mode in ["write", "read"] {
        let (control, workspace, store, provider) = setup(&binary, mode).await;
        let result = provider
            .execute(
                ProviderExecutionContext {
                    execution_id: "e".into(),
                },
                Arc::new(Sink(control.path().into())),
                Arc::new(SlowTelemetry),
            )
            .await
            .unwrap();
        assert_eq!(result.outcome, ProviderOutcome::Completed);
        assert_eq!(result.result, Some(json!({"text":"safe result"})));
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(row.status, "completed");
        assert_eq!(row.provider_terminal_status.as_deref(), Some("completed"));
        assert_eq!(
            row.final_result_json,
            Some(serde_json::to_string(&result.result).unwrap())
        );
        assert_eq!(row.result_completeness, "complete");
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
        let release: Value =
            serde_json::from_str(row.release_evidence_json.as_ref().unwrap()).unwrap();
        assert_eq!(release["runtime_instance_id"], runtime.id);
        assert_eq!(
            release["evidence_at"],
            runtime.termination_evidence_at.unwrap()
        );
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_none()
        );
        let db = rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap();
        let trace: Vec<(String, String, i64, String, String)> = db
            .prepare("SELECT * FROM cb7_trace")
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            trace
                .iter()
                .any(|r| r.0 == "finalizing" && r.2 == 1 && r.3 == "running" && r.4 != "complete")
        );
        let dispatching = trace.iter().position(|r| r.1 == "dispatching").unwrap();
        let dispatched = trace.iter().position(|r| r.1 == "dispatched").unwrap();
        let running = trace.iter().position(|r| r.0 == "running").unwrap();
        let terminal = trace.iter().position(|r| r.0 == "finalizing").unwrap();
        assert!(dispatching < dispatched && dispatched < running && running < terminal);
        let files: Vec<_> = std::fs::read_dir(workspace.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        if mode == "write" {
            assert_eq!(files, vec![std::ffi::OsString::from("output.txt")]);
            assert_eq!(
                std::fs::read(workspace.path().join("output.txt")).unwrap(),
                b"CB7_005_WRITE\n"
            );
        } else {
            assert!(files.is_empty());
        }
    }
}

#[tokio::test]
/// 无 Provider terminal 时安全终止只能 Interrupted；证据持久化失败必须 Unknown + Claim。
async fn native_execute_eof_and_evidence_persistence_failure() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for failure in [false, true] {
        let (control, _workspace, store, provider) =
            setup(&binary, if failure { "read" } else { "eof" }).await;
        if failure {
            rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap().execute_batch("CREATE TRIGGER fail_evidence BEFORE UPDATE OF termination_evidence_state ON runtime_instances WHEN NEW.termination_evidence_state='complete' BEGIN SELECT RAISE(ABORT,'evidence fault'); END;").unwrap();
        }
        assert!(
            provider
                .execute(
                    ProviderExecutionContext {
                        execution_id: "e".into()
                    },
                    Arc::new(Sink(control.path().into())),
                    Arc::new(SlowTelemetry)
                )
                .await
                .is_err()
        );
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(row.status, if failure { "unknown" } else { "interrupted" });
        assert_eq!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_some(),
            failure
        );
        if failure {
            assert_eq!(row.provider_terminal_status.as_deref(), Some("completed"));
            assert_eq!(
                row.final_result_json.as_deref(),
                Some("{\"text\":\"safe result\"}")
            );
            rusqlite::Connection::open(control.path().join("agent-state.db"))
                .unwrap()
                .execute_batch("DROP TRIGGER fail_evidence")
                .unwrap();
            provider
                .startup_reconcile(ProviderStartupContext {})
                .await
                .unwrap();
            let resumed = store.execution("e".into()).await.unwrap().unwrap();
            assert_eq!(resumed.status, "completed");
            assert_eq!(resumed.final_result_json, row.final_result_json);
        } else {
            assert!(row.provider_terminal_status.is_none());
            assert_eq!(row.result_completeness, "unknown");
        }
    }
}

#[tokio::test]
/// host crash window：已暂存 exact terminal 时 Job 尚活，startup 终止后保留安全结果。
async fn native_staged_live_job_startup_preserves_terminal_and_result() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "read").await;
    let session = provider
        .prepare_fresh("e".into(), DesiredConfiguration::default())
        .await
        .unwrap();
    let completion = crate::agent::codebuddy::prompt::prompt(
        session,
        store.clone(),
        Arc::new(Sink(control.path().into())),
        Arc::new(SlowTelemetry),
    )
    .await
    .unwrap();
    let result = completion.result.unwrap();
    let runtime = completion
        .session
        .private
        .runtime_instance_id
        .clone()
        .unwrap();
    store
        .provider_event(
            "e".into(),
            Transition::ProviderTerminalResult {
                runtime_id: runtime.clone(),
                status: Status::Completed,
                result: result.result.clone(),
                completeness: ResultCompleteness::Complete,
            },
            now(),
        )
        .await
        .unwrap();
    let staged = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(staged.status, "finalizing");
    assert_eq!(staged.release_evidence_state, "incomplete");
    assert!(
        store
            .workspace_claim(staged.canonical_workspace_root.clone())
            .await
            .unwrap()
            .is_some()
    );
    let live = store.runtime(runtime.clone()).await.unwrap().unwrap();
    assert_eq!(live.state, "running");
    assert_ne!(live.termination_evidence_state, "complete");
    assert!(active_job_processes(live.job_name.as_deref().unwrap()) > 0);
    // CLI 缺失的 registered skeleton 仍恢复当前 owned Runtime；不重新 launch。
    let missing = CodeBuddyProvider::from_discovery(
        store.clone(),
        "new-host".into(),
        Err(crate::agent::codebuddy::discovery::DiscoveryError::not_found(false)),
    );
    missing
        .startup_reconcile(ProviderStartupContext {})
        .await
        .unwrap();
    let done = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(done.status, "completed");
    assert_eq!(done.final_result_json, staged.final_result_json);
    assert_eq!(
        done.provider_terminal_status,
        staged.provider_terminal_status
    );
    assert!(
        store
            .workspace_claim(done.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .runtime(runtime)
            .await
            .unwrap()
            .unwrap()
            .termination_evidence_state,
        "complete"
    );
    drop(completion.session);
}

#[tokio::test]
/// worker caller drop 不丢失 Runtime owner，也不能伪造 Provider terminal。
async fn native_caller_drop_after_flush_converges_without_replay() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "hold").await;
    let sink = Arc::new(Sink(control.path().into()));
    let task = tokio::spawn(async move {
        provider
            .execute(
                ProviderExecutionContext {
                    execution_id: "e".into(),
                },
                sink,
                Arc::new(SlowTelemetry),
            )
            .await
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while !control.path().join("prompt.json").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    task.abort();
    let _ = task.await;
    loop {
        let row = store.execution("e".into()).await.unwrap().unwrap();
        if row.status == "interrupted" {
            assert!(row.provider_terminal_status.is_none());
            assert_eq!(row.result_completeness, "unknown");
            assert!(
                store
                    .workspace_claim(row.canonical_workspace_root)
                    .await
                    .unwrap()
                    .is_none()
            );
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{}", row.status);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        store
            .read_codebuddy_state("e".into())
            .await
            .unwrap()
            .prompt_state,
        crate::agent::codebuddy::store::PromptState::Uncertain
    );
}

/// 真实 QueryInformationJobObject 仅用于 native crash-window 断言，不制造持久化证据。
fn active_job_processes(name: &str) -> u32 {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::{JobObjects::*, SystemServices::JOB_OBJECT_QUERY};
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: NUL 结尾名称、owned handle 与匹配的 Win32 输出缓冲区。
    unsafe {
        let handle = OpenJobObjectW(JOB_OBJECT_QUERY, 0, name.as_ptr());
        assert!(!handle.is_null());
        let handle = OwnedHandle::from_raw_handle(handle);
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
        assert_ne!(
            QueryInformationJobObject(
                handle.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut()
            ),
            0
        );
        info.ActiveProcesses
    }
}

#[tokio::test]
/// 真实 pipe 在 acceptance 前关闭输入，但 stdout 保持；物理 flush 失败收敛 Uncertain。
async fn native_post_accept_preflush_failure_is_uncertain_then_interrupted() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "closed-input").await;
    assert!(
        provider
            .execute(
                ProviderExecutionContext {
                    execution_id: "e".into()
                },
                Arc::new(Sink(control.path().into())),
                Arc::new(SlowTelemetry)
            )
            .await
            .is_err()
    );
    assert!(control.path().join("accepted").exists());
    assert!(!control.path().join("prompt.json").exists());
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(row.status, "interrupted");
    assert_eq!(row.dispatch_state, "uncertain");
    assert!(row.provider_terminal_status.is_none());
    assert_eq!(
        store
            .read_codebuddy_state("e".into())
            .await
            .unwrap()
            .prompt_state,
        crate::agent::codebuddy::store::PromptState::Uncertain
    );
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
/// 两个 bypass execute 竞争同一 Execution，输家不得收敛或停止赢家的 R1。
async fn native_competing_execute_only_cleans_its_own_attempt() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "read").await;
    let sink = Arc::new(Sink(control.path().into()));
    let context = ProviderExecutionContext {
        execution_id: "e".into(),
    };
    let (a, b) = tokio::join!(
        provider.execute(context.clone(), sink.clone(), Arc::new(SlowTelemetry)),
        provider.execute(context, sink, Arc::new(SlowTelemetry))
    );
    assert_ne!(a.is_ok(), b.is_ok(), "{a:?} {b:?}");
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(row.status, "completed");
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
    let count: i64 = rusqlite::Connection::open(control.path().join("agent-state.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM runtime_instances", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
/// registered adapter 持有当前生命周期；accepted 后 disable 只阻止后续 admission。
async fn native_disable_after_acceptance_preserves_owned_execute() {
    use crate::agent::provider::{
        ProviderErrorCode, ProviderId,
        control::{ProviderAdmissionCapability, ProviderAdmissionPolicy},
        registry::{ProviderHealth, ProviderRegistry},
    };
    struct DisableSink {
        sink: Sink,
        policy: ProviderAdmissionPolicy,
    }
    impl ProviderAcceptanceSink for DisableSink {
        /// 回调时改变实际 shared policy，当前 execution 不得重读它进行取消。
        fn accepted(&self) {
            self.sink.accepted();
            self.policy.set_enabled_for_test("codebuddy", false);
        }
    }
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "read").await;
    let mut registry = ProviderRegistry::new();
    registry
        .register(Arc::new(provider), ProviderHealth::Available)
        .unwrap();
    let policy = ProviderAdmissionPolicy::new(Default::default());
    policy.set_enabled_for_test("codebuddy", true);
    let id = ProviderId::new("codebuddy".into()).unwrap();
    let provider = policy
        .admit(&registry, &id, ProviderAdmissionCapability::Execute)
        .unwrap();
    provider
        .execute(
            ProviderExecutionContext {
                execution_id: "e".into(),
            },
            Arc::new(DisableSink {
                sink: Sink(control.path().into()),
                policy: policy.clone(),
            }),
            Arc::new(SlowTelemetry),
        )
        .await
        .unwrap();
    assert_eq!(
        store.execution("e".into()).await.unwrap().unwrap().status,
        "completed"
    );
    match policy.admit(&registry, &id, ProviderAdmissionCapability::Execute) {
        Err(error) => assert_eq!(error.code, ProviderErrorCode::AgentProviderDisabled),
        Ok(_) => panic!("disabled provider admitted new execution"),
    }
}

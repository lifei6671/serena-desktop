//! CB3-005：复用真实 Broker fixture，验证冻结身份、恢复准入与公开契约。
use super::*;
use crate::agent::provider::port::{ProviderContinuationContext, ProviderContinuationDecision};
use std::sync::Mutex;

struct ContinuationProvider {
    inner: Arc<RoutingProvider>,
    can_continue: bool,
    eligible: bool,
    can_cancel: bool,
    validated: Mutex<Vec<String>>,
    cancelled: Mutex<Vec<String>>,
}

impl AgentProvider for ContinuationProvider {
    /// 复用源 Provider 身份，不借用当前 role route。
    fn descriptor(&self) -> ProviderDescriptor {
        self.inner.descriptor()
    }
    /// 未验证的 recover/usage 能力始终为 false。
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_continue: self.can_continue,
            can_cancel: self.can_cancel,
            ..self.inner.capabilities()
        }
    }
    /// 复用无进程执行 fixture，统计真正派发到源 Provider 的次数。
    fn execute<'a>(
        &'a self,
        context: ProviderExecutionContext,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
        telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        self.inner.execute(context, acceptance, telemetry)
    }
    /// 记录原 Execution 身份，再使用现有 Store cancel authority。
    fn cancel<'a>(
        &'a self,
        context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async move {
            self.cancelled
                .lock()
                .unwrap()
                .push(context.execution_id.clone());
            self.inner
                .store
                .request_cancel(context.execution_id, crate::agent::coordinator::now())
                .await
                .unwrap();
            Ok(())
        })
    }
    /// 验证实际传入 sourceExecutionId，而非 child 或当前路由的新身份。
    fn validate_continuation<'a>(
        &'a self,
        context: ProviderContinuationContext,
    ) -> ProviderFuture<'a, Result<ProviderContinuationDecision, ProviderError>> {
        Box::pin(async move {
            self.validated
                .lock()
                .unwrap()
                .push(context.source_execution_id);
            Ok(if self.eligible {
                ProviderContinuationDecision::Eligible
            } else {
                ProviderContinuationDecision::Ineligible
            })
        })
    }
    /// 本矩阵不启动恢复 worker。
    fn startup_reconcile<'a>(
        &'a self,
        context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        self.inner.startup_reconcile(context)
    }
}

/// 替换共享 Registry 的测试 adapter，不改变任何持久化 Execution identity。
fn install(
    f: &RoutingFixture,
    health: ProviderHealth,
    can_continue: bool,
    eligible: bool,
    can_cancel: bool,
) -> Arc<ContinuationProvider> {
    let provider = Arc::new(ContinuationProvider {
        inner: f.provider.clone(),
        can_continue,
        eligible,
        can_cancel,
        validated: Mutex::new(Vec::new()),
        cancelled: Mutex::new(Vec::new()),
    });
    let mut registry = ProviderRegistry::new();
    registry.register(provider.clone(), health).unwrap();
    f.manager.clone().use_registry(registry);
    provider
}

/// 比较完整行，包含 dispatch/bind 字段与独立、不可变的 Runtime attempt ledger。
fn evidence(f: &RoutingFixture) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    let mut rows = f.rows();
    let db = rusqlite::Connection::open(f.state_root.join("agent-state.db")).unwrap();
    for table in ["execution_runtime_attempts", "work_execution_links"] {
        let mut query = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = query.column_count();
        rows.push(
            query
                .query_map([], |row| (0..columns).map(|index| row.get(index)).collect())
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
        );
    }
    rows
}

/// 等待无进程 fixture 的真实 Store terminal 提交，不把 acceptance 当作终态。
async fn terminal(f: &RoutingFixture, id: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if f.store.execution(id.into()).await.unwrap().unwrap().status == "cancelled" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

/// 通过公开 Start 创建非 General 的合法 terminal source。
async fn source(f: &RoutingFixture, work: &str) -> String {
    let response = f.execute(start(work)).await;
    assert_eq!(response["ok"], true, "{response}");
    let id = f.store.product_read(None, None, None, 10).await.unwrap()[0]
        .execution
        .id
        .clone();
    terminal(f, &id).await;
    id
}

/// 公共 Continue 只带 Work/source/request identity。
fn continuation(work: &str, source: &str) -> Value {
    json!({"action":"continue","workRunId":work,"parentExecutionId":source,"requestKey":"child","prompt":"继续验证"})
}

/// 在真实创建后、首次派发前停住并禁用 Provider，形成合法 pending + Claim。
async fn pending() -> (RoutingFixture, String, String) {
    let handoff = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
    let f = RoutingFixture::new(ProviderHealth::Available, true, Some(handoff.clone())).await;
    let work = f.begin().await;
    let broker = f.broker.clone();
    let request = start(&work);
    let task = tokio::spawn(async move {
        broker
            .orchestration_operation("agent_execute", request)
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), handoff.0.notified())
        .await
        .unwrap();
    f.policy(|settings| {
        settings.providers.get_mut("fixture").unwrap().enabled = false;
    })
    .await;
    handoff.1.notify_one();
    let response = task.await.unwrap();
    assert_eq!(
        response["error"]["code"], "AGENT_PROVIDER_DISABLED",
        "{response}"
    );
    let row = f
        .store
        .product_read(None, None, None, 10)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(row.execution.status, "dispatch_pending");
    assert_eq!(row.execution.dispatch_state, "not_dispatched");
    assert!(row.owns_claim);
    assert!(!row.runtime_attempt_exists);
    (f, work, row.execution.id)
}

/// 当前 route 改变后，child 仍继承 source 的 Provider/Role/完整工作区与 profile。
#[tokio::test]
async fn continue_inherits_frozen_identity_while_query_reports_current_route() {
    let f = RoutingFixture::new(ProviderHealth::Available, true, None).await;
    let work = f.begin().await;
    let id = source(&f, &work).await;
    let provider = install(&f, ProviderHealth::Available, true, true, true);
    f.policy(|settings| {
        settings.role_routing.insert(
            "testing".into(),
            Some(ProviderId::new("other".into()).unwrap()),
        );
    })
    .await;
    let before = f
        .store
        .product_read(Some(id.clone()), None, None, 1)
        .await
        .unwrap()
        .remove(0);
    // routing error 后目录仍可读；既有 source 的 Continue 不迁移到当前 route。
    assert_eq!(
        f.execute(start(&work)).await["error"]["code"],
        "AGENT_ROLE_PROVIDER_MISMATCH"
    );
    let current = f
        .broker
        .orchestration_operation("agent_query", json!({"action":"providers"}))
        .await;
    assert_eq!(current["ok"], true);
    assert_eq!(current["data"]["roleRouting"]["testing"], "other");
    let response = f.execute(continuation(&work, &id)).await;
    assert_eq!(response["ok"], true, "{response}");
    let rows = f.store.product_read(None, None, None, 10).await.unwrap();
    assert_eq!(rows.len(), 2);
    let child = rows.iter().find(|row| row.execution.id != id).unwrap();
    terminal(&f, &child.execution.id).await;
    assert_eq!(
        f.store.execution(id.clone()).await.unwrap(),
        Some(before.execution.clone())
    );
    assert_eq!(
        child.execution.parent_execution_id.as_deref(),
        Some(id.as_str())
    );
    assert_eq!(child.execution.provider, before.execution.provider);
    assert_eq!(child.task_role, "testing");
    assert_eq!(child.task_role, before.task_role);
    assert_eq!(child.execution.workspace_id, before.execution.workspace_id);
    assert_eq!(
        child.execution.canonical_workspace_root,
        before.execution.canonical_workspace_root
    );
    assert_eq!(
        child.execution.workspace_generation,
        before.execution.workspace_generation
    );
    assert_eq!(
        child.execution.execution_profile_json,
        before.execution.execution_profile_json
    );
    assert!(
        provider
            .validated
            .lock()
            .unwrap()
            .iter()
            .all(|value| value == &id || value == &child.execution.id)
    );
    assert!(provider.validated.lock().unwrap().contains(&id));
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 2);
    let detail = f
        .broker
        .orchestration_operation("agent_query", json!({"action":"get","executionId":id}))
        .await;
    assert_eq!(detail["ok"], true, "{detail}");
    assert_eq!(detail["data"]["provider"]["id"], "fixture");
    assert_eq!(detail["data"]["taskRole"], "testing");
    let catalog = f
        .broker
        .orchestration_operation("agent_query", json!({"action":"providers"}))
        .await;
    assert_eq!(catalog["data"]["roleRouting"]["testing"], "other");
    assert_query_output_contract(&[detail, catalog]);

    // Store 的通用 Provider 比较仍必须阻止同一 lineage 偷换 Provider。
    let persisted = evidence(&f);
    let forged = serde_json::from_value(json!({
        "agent_id":before.execution.agent_id,
        "request_key":"forged-provider",
        "prompt":"不得跨 Provider 继续",
        "execution_profile":{},
        "workspace_id":before.execution.workspace_id,
        "canonical_workspace_root":before.execution.canonical_workspace_root,
        "workspace_generation":before.execution.workspace_generation,
        "provider":"other",
        "task_role":"testing",
        "mode":"workspace_write",
        "parent_execution_id":before.execution.id
    }))
    .unwrap();
    let error = f
        .store
        .create_execution(
            "forged-child".into(),
            crate::agent::execution::canonicalize_request(forged).unwrap(),
            crate::agent::coordinator::now(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, "AGENT_SNAPSHOT_CONFLICT");
    assert_eq!(evidence(&f), persisted);
}

/// 每项拒绝均在 child/Claim/Runtime/attempt/link 创建之前发生；无 fresh Start fallback。
#[tokio::test]
async fn continue_admission_rejections_preserve_all_persisted_evidence() {
    for (case, code) in [
        ("missing", "AGENT_CONTINUE_NOT_ALLOWED"),
        ("disabled", "AGENT_PROVIDER_DISABLED"),
        ("unavailable", "AGENT_PROVIDER_UNAVAILABLE"),
        ("capability", "AGENT_CONTINUE_NOT_ALLOWED"),
        ("validation", "AGENT_CONTINUE_NOT_ALLOWED"),
    ] {
        let f = RoutingFixture::new(ProviderHealth::Available, true, None).await;
        let work = f.begin().await;
        let id = source(&f, &work).await;
        let provider = install(
            &f,
            if case == "unavailable" {
                ProviderHealth::Unavailable
            } else {
                ProviderHealth::Available
            },
            case != "capability",
            case != "validation",
            true,
        );
        if case == "missing" {
            f.manager.clone().use_registry(ProviderRegistry::new());
        }
        f.policy(|settings| {
            settings.role_routing.insert(
                "testing".into(),
                Some(ProviderId::new("other".into()).unwrap()),
            );
            if case == "disabled" {
                settings.providers.get_mut("fixture").unwrap().enabled = false;
            }
        })
        .await;
        let before = evidence(&f);
        let response = f.execute(continuation(&work, &id)).await;
        assert_eq!(response["error"]["code"], code, "{case}: {response}");
        assert_eq!(evidence(&f), before, "{case}");
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
        if case != "validation" {
            assert!(provider.validated.lock().unwrap().is_empty(), "{case}");
        }
        let catalog = f
            .broker
            .orchestration_operation("agent_query", json!({"action":"providers"}))
            .await;
        assert_eq!(catalog["ok"], true, "{catalog}");
        assert_eq!(catalog["data"]["roleRouting"]["testing"], "other");
        if case == "capability" {
            let caps = &catalog["data"]["providers"][0]["capabilities"];
            for name in ["canContinue", "canRecover", "tokenUsage"] {
                assert_eq!(caps[name], false, "{catalog}");
            }
        }
    }
}

/// disabled/unavailable 不改原行；重新启用后派发原 Execution，忽略当前路由变化。
#[tokio::test]
async fn resume_rejections_preserve_claim_then_reenable_dispatches_same_identity() {
    let (f, work, id) = pending().await;
    f.policy(|settings| {
        settings.role_routing.insert(
            "testing".into(),
            Some(ProviderId::new("other".into()).unwrap()),
        );
    })
    .await;
    let request = json!({"action":"resume_pending","workRunId":work,"executionId":id});
    let before = evidence(&f);
    assert_eq!(
        f.execute(request.clone()).await["error"]["code"],
        "AGENT_PROVIDER_DISABLED"
    );
    assert_eq!(evidence(&f), before);
    f.policy(|settings| {
        settings.providers.get_mut("fixture").unwrap().enabled = true;
    })
    .await;
    install(&f, ProviderHealth::Unavailable, false, false, true);
    assert_eq!(
        f.execute(request.clone()).await["error"]["code"],
        "AGENT_PROVIDER_UNAVAILABLE"
    );
    assert_eq!(evidence(&f), before);
    install(&f, ProviderHealth::Available, false, false, true);
    let response = f.execute(request).await;
    assert_eq!(response["ok"], true, "{response}");
    terminal(&f, &id).await;
    let rows = f.store.product_read(None, None, None, 10).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].execution.id, id);
    assert_eq!(rows[0].execution.provider, "fixture");
    assert_eq!(rows[0].task_role, "testing");
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
}

/// disabled 不能清除已有 Runtime attempt；重新启用也不能绕过首次派发安全门禁。
#[tokio::test]
async fn resume_keeps_runtime_attempt_evidence_and_rejects_replay() {
    let (f, work, id) = pending().await;
    let db = rusqlite::Connection::open(f.state_root.join("agent-state.db")).unwrap();
    db.execute("INSERT INTO execution_runtime_attempts(execution_id,runtime_instance_id,created_at) VALUES (?1,'attempt',1)", [&id]).unwrap();
    let request = json!({"action":"resume_pending","workRunId":work,"executionId":id});
    let before = evidence(&f);
    assert_eq!(
        f.execute(request.clone()).await["error"]["code"],
        "AGENT_PROVIDER_DISABLED"
    );
    assert_eq!(evidence(&f), before);
    f.policy(|settings| {
        settings.providers.get_mut("fixture").unwrap().enabled = true;
    })
    .await;
    let response = f.execute(request).await;
    assert_eq!(
        response["error"]["code"], "AGENT_RESUME_NOT_ALLOWED",
        "{response}"
    );
    assert_eq!(evidence(&f), before);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
}

/// Cancel 忽略 enabled/health，但仍要求 registered + canCancel。
#[tokio::test]
async fn cancel_remains_registration_only_under_disabled_unavailable_policy() {
    let (f, work, id) = pending().await;
    let request = json!({"action":"cancel","workRunId":work,"executionId":id});
    let before = evidence(&f);
    f.manager.clone().use_registry(ProviderRegistry::new());
    assert_eq!(
        f.manager.cancel(&id).await.unwrap_err(),
        "AGENT_PROVIDER_NOT_FOUND"
    );
    // 公共 Cancel 沿用既有通用错误投影，具体拒绝事实由 TaskManager 保留。
    assert_eq!(
        f.execute(request.clone()).await["error"]["code"],
        "AGENT_OPERATION_FAILED"
    );
    assert_eq!(evidence(&f), before);
    let incapable = install(&f, ProviderHealth::Unavailable, false, false, false);
    assert_eq!(
        f.manager.cancel(&id).await.unwrap_err(),
        "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED"
    );
    assert_eq!(
        f.execute(request.clone()).await["error"]["code"],
        "AGENT_OPERATION_FAILED"
    );
    assert!(incapable.cancelled.lock().unwrap().is_empty());
    assert_eq!(evidence(&f), before);
    let provider = install(&f, ProviderHealth::Unavailable, false, false, true);
    let response = f.execute(request).await;
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(*provider.cancelled.lock().unwrap(), vec![id.clone()]);
    terminal(&f, &id).await;
    let row = f
        .store
        .product_read(Some(id), None, None, 1)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(row.execution.provider, "fixture");
    assert_eq!(row.task_role, "testing");
    assert!(!row.owns_claim);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
}

/// schema 与真实 parser 都拒绝在后续动作注入 workspace/provider/role 身份。
#[test]
fn continuation_cancel_resume_strict_schema_and_parser_reject_identity_fields() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let valid = vec![
        continuation("w", "e"),
        json!({"action":"cancel","workRunId":"w","executionId":"e"}),
        json!({"action":"resume_pending","workRunId":"w","executionId":"e"}),
    ];
    let mut invalid = Vec::new();
    for request in &valid {
        assert!(orchestration::validate("agent_execute", request).is_ok());
        for (field, value) in [
            ("workspaceId", "W"),
            ("providerId", "fixture"),
            ("taskRole", "testing"),
        ] {
            let mut changed = request.clone();
            changed[field] = json!(value);
            assert!(
                orchestration::validate("agent_execute", &changed).is_err(),
                "{changed}"
            );
            invalid.push(changed);
        }
    }
    let tool = orchestration::descriptors()
        .into_iter()
        .find(|tool| tool.name == "agent_execute")
        .unwrap();
    let mut child = Command::new("node").current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).args(["-e", r#"
const Ajv = require('ajv'); let text = '';
process.stdin.on('data', chunk => text += chunk);
process.stdin.on('end', () => {
  const {schema, valid, invalid} = JSON.parse(text); delete schema.$schema;
  const check = new Ajv({allErrors:true}).compile(schema);
  for (const value of valid) if (!check(value)) throw Error(JSON.stringify(check.errors));
  for (const value of invalid) if (check(value)) throw Error('forbidden identity accepted: '+JSON.stringify(value));
});
"#]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"schema":tool.input_schema,"valid":valid,"invalid":invalid})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

use super::*;
use crate::agent::execution::ExecutionProfile;

fn profile(model: &str, reasoning: &str) -> ExecutionProfile {
    ExecutionProfile {
        model: Some(model.into()),
        reasoning: Some(reasoning.into()),
    }
}

fn bind(store: &StateStore, execution_id: &str, runtime_id: &str, runtime_provider: &str) {
    let connection = store.connection.lock().unwrap();
    connection
        .execute(
            "INSERT INTO runtime_instances
             (id,owner_host_instance_id,provider,state,created_at,updated_at)
             VALUES (?1,'host',?2,'running',1,1)",
            params![runtime_id, runtime_provider],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE executions SET runtime_instance_id=?2 WHERE id=?1",
            params![execution_id, runtime_id],
        )
        .unwrap();
}

fn lifecycle_snapshot(store: &StateStore, execution_id: &str) -> (String, String, i64, i64, i64) {
    let connection = store.connection.lock().unwrap();
    connection
        .query_row(
            "SELECT status,dispatch_state,revision,updated_at,
                    (SELECT count(*) FROM workspace_claims WHERE execution_id=?1)
             FROM executions WHERE id=?1",
            [execution_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap()
}

/// 首写与相同值重试幂等，不改变请求快照或任何生命周期/Claim 证据。
#[tokio::test]
async fn effective_profile_first_write_same_retry_and_conflict_are_immutable() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    store
        .create_execution("execution".into(), request("agent", "root"), 7)
        .await
        .unwrap();
    bind(&store, "execution", "runtime", "codex");
    let before = lifecycle_snapshot(&store, "execution");

    let effective = profile("actual-model", "high");
    store
        .set_effective_execution_profile(
            "execution".into(),
            "codex".into(),
            "runtime".into(),
            effective.clone(),
        )
        .await
        .unwrap();
    store
        .set_effective_execution_profile(
            "execution".into(),
            "codex".into(),
            "runtime".into(),
            effective.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .set_effective_execution_profile(
                "execution".into(),
                "codex".into(),
                "runtime".into(),
                profile("other-model", "high"),
            )
            .await
            .unwrap_err(),
        "EFFECTIVE_EXECUTION_PROFILE_CONFLICT"
    );

    let row = store.execution("execution".into()).await.unwrap().unwrap();
    assert_eq!(
        ExecutionProfile::from_json(row.effective_execution_profile_json.as_deref().unwrap())
            .unwrap(),
        effective
    );
    assert_eq!(
        ExecutionProfile::from_json(&row.execution_profile_json).unwrap(),
        profile("fixture-model", "high")
    );
    assert_eq!(lifecycle_snapshot(&store, "execution"), before);
}

/// 写入口拒绝空配置以及 Execution/Runtime/Provider 任一身份不匹配。
#[tokio::test]
async fn effective_profile_rejects_empty_and_identity_mismatches() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    store
        .create_execution("execution".into(), request("agent", "root"), 1)
        .await
        .unwrap();
    bind(&store, "execution", "runtime", "codex");
    {
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO runtime_instances
                 (id,owner_host_instance_id,provider,state,created_at,updated_at)
                 VALUES ('other-runtime','host','codex','running',1,1)",
                [],
            )
            .unwrap();
    }

    let cases = [
        (
            store
                .set_effective_execution_profile(
                    "execution".into(),
                    "codex".into(),
                    "runtime".into(),
                    profile("\t", "high"),
                )
                .await,
            "EFFECTIVE_EXECUTION_PROFILE_INVALID",
        ),
        (
            store
                .set_effective_execution_profile(
                    "execution".into(),
                    "codex".into(),
                    "runtime".into(),
                    ExecutionProfile::default(),
                )
                .await,
            "EFFECTIVE_EXECUTION_PROFILE_INCOMPLETE",
        ),
        (
            store
                .set_effective_execution_profile(
                    "execution".into(),
                    "codebuddy".into(),
                    "runtime".into(),
                    profile("model", "high"),
                )
                .await,
            "EFFECTIVE_EXECUTION_PROFILE_PROVIDER_MISMATCH",
        ),
        (
            store
                .set_effective_execution_profile(
                    "execution".into(),
                    "codex".into(),
                    "other-runtime".into(),
                    profile("model", "high"),
                )
                .await,
            "EFFECTIVE_EXECUTION_PROFILE_RUNTIME_MISMATCH",
        ),
        (
            store
                .set_effective_execution_profile(
                    "missing".into(),
                    "codex".into(),
                    "runtime".into(),
                    profile("model", "high"),
                )
                .await,
            "EXECUTION_NOT_FOUND",
        ),
    ];
    for (result, expected) in cases {
        assert_eq!(result.unwrap_err(), expected);
    }
    assert!(
        store
            .execution("execution".into())
            .await
            .unwrap()
            .unwrap()
            .effective_execution_profile_json
            .is_none()
    );

    store
        .create_execution(
            "provider-mismatch".into(),
            request("other-agent", "other-root"),
            2,
        )
        .await
        .unwrap();
    bind(
        &store,
        "provider-mismatch",
        "codebuddy-runtime",
        "codebuddy",
    );
    assert_eq!(
        store
            .set_effective_execution_profile(
                "provider-mismatch".into(),
                "codex".into(),
                "codebuddy-runtime".into(),
                profile("model", "high"),
            )
            .await
            .unwrap_err(),
        "EFFECTIVE_EXECUTION_PROFILE_RUNTIME_PROVIDER_MISMATCH"
    );
}

/// model-only 与 reasoning-only 都是可持久化的独立 Provider 事实；仅 `{}` 非法。
#[tokio::test]
async fn effective_profile_accepts_each_independently_known_field() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    for (id, root, runtime, effective) in [
        (
            "model-only",
            "model-root",
            "model-runtime",
            ExecutionProfile {
                model: Some("actual-model".into()),
                reasoning: None,
            },
        ),
        (
            "reasoning-only",
            "reasoning-root",
            "reasoning-runtime",
            ExecutionProfile {
                model: None,
                reasoning: Some("high".into()),
            },
        ),
    ] {
        store
            .create_execution(id.into(), request(id, root), 1)
            .await
            .unwrap();
        bind(&store, id, runtime, "codex");
        store
            .set_effective_execution_profile(
                id.into(),
                "codex".into(),
                runtime.into(),
                effective.clone(),
            )
            .await
            .unwrap();
        let stored = store.execution(id.into()).await.unwrap().unwrap();
        assert_eq!(
            ExecutionProfile::from_json(
                stored.effective_execution_profile_json.as_deref().unwrap()
            )
            .unwrap(),
            effective
        );
    }
}

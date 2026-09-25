use super::*;
use crate::agent::{
    execution::{CreateExecutionInput, canonicalize_request},
    provider::registry::ProviderRegistry,
};

/// 用真实 Store 创建带冻结角色的 v3 Execution，不经过尚未实现的角色路由。
async fn create_with_role(store: &StateStore, id: &str, role: &str, mode: &str) {
    let input: CreateExecutionInput = serde_json::from_value(json!({
        "agent_id": format!("agent-{id}"),
        "request_key": format!("key-{id}"),
        "prompt": "role fixture",
        "execution_profile": {},
        "workspace_id": format!("workspace-{id}"),
        "canonical_workspace_root": format!("C:/role-{id}"),
        "workspace_generation": 1,
        "provider": "codex",
        "task_role": role,
        "mode": mode
    }))
    .unwrap();
    store
        .create_execution(id.into(), canonicalize_request(input).unwrap(), 10)
        .await
        .unwrap();
}

/// 五个冻结 wire 值都由 Store 原值投影；Registry 变化不能改写历史角色。
#[tokio::test]
async fn persisted_roles_project_identically_in_detail_query_observe_and_list() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    for role in ["development", "testing", "review", "analysis", "general"] {
        create_with_role(&store, role, role, "read_only").await;
    }
    let mut service = AgentProductService::new(store);
    for role in ["development", "testing", "review", "analysis", "general"] {
        let detail =
            serde_json::to_value(service.observe(role.into(), false).await.unwrap()).unwrap();
        let query = success(
            service
                .agent_query(AgentQueryAction::Get {
                    execution_id: role.into(),
                    include_result: Some(false),
                })
                .await
                .unwrap(),
        );
        let observed = service
            .operation(
                json!({"action":"observe","executionId":role,"waitMs":0}),
                None,
            )
            .await;
        let listed = service.operation(json!({"action":"list"}), None).await;
        let in_list = listed["data"]["executions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["executionId"] == role)
            .unwrap();
        for view in [&detail, &query["data"], &observed["data"], in_list] {
            assert_eq!(view["provider"]["id"], "codex");
            assert_eq!(view["taskRole"], role);
        }
    }

    // 当前 Registry 可改变展示元数据，但不能作为历史 taskRole 的 Authority。
    let before = service.observe("testing".into(), false).await.unwrap();
    service.manager.use_registry(ProviderRegistry::new());
    let after = service.observe("testing".into(), false).await.unwrap();
    assert_eq!(before.task_role, after.task_role);
    assert_eq!(before.control_revision, after.control_revision);
    assert_eq!(before.activity_revision, after.activity_revision);
    assert_eq!(after.task_role, "testing");
}

/// 非法持久化值产生稳定 contract error，不能被 Product 默认为 General。
#[tokio::test]
async fn invalid_persisted_role_fails_product_contract() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_with_role(&store, "invalid", "general", "read_only").await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute_batch(
        "PRAGMA ignore_check_constraints=ON;\
         UPDATE executions SET task_role='unrecognized' WHERE id='invalid';\
         PRAGMA ignore_check_constraints=OFF;",
    )
    .unwrap();
    let service = AgentProductService::new(store);
    for action in [
        json!({"action":"observe","executionId":"invalid","waitMs":0}),
        json!({"action":"list"}),
    ] {
        let response = service.operation(action, None).await;
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "AGENT_TASK_ROLE_CONTRACT_ERROR");
    }
    let queried = service
        .agent_query(AgentQueryAction::Get {
            execution_id: "invalid".into(),
            include_result: Some(false),
        })
        .await
        .err()
        .unwrap();
    assert_eq!(queried.code, "AGENT_TASK_ROLE_CONTRACT_ERROR");
}

/// Continue 的 Product 仅显示 Store 已从父行继承并持久化到子行的角色。
#[tokio::test]
async fn continuation_child_projects_persisted_parent_role() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_with_role(&store, "parent", "testing", "workspace_write").await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute(
        "UPDATE executions SET status='completed',dispatch_state='dispatched',\
         release_evidence_state='complete',release_evidence_kind='same_runtime_cleanup',\
         release_evidence_json='{}',completed_at=11 WHERE id='parent'",
        [],
    )
    .unwrap();
    db.execute(
        "DELETE FROM workspace_claims WHERE execution_id='parent'",
        [],
    )
    .unwrap();
    let child = store
        .product_create_continuation(
            "child".into(),
            "parent".into(),
            "child-key".into(),
            "child prompt".into(),
            12,
        )
        .await
        .unwrap();
    assert!(child.created);
    let persisted: String = db
        .query_row(
            "SELECT task_role FROM executions WHERE id='child'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(persisted, "testing");
    let service = AgentProductService::new(store);
    let parent = service.observe("parent".into(), false).await.unwrap();
    let child = service.observe("child".into(), false).await.unwrap();
    assert_eq!(parent.task_role, "testing");
    assert_eq!(child.task_role, persisted);
    assert_eq!(child.provider.id, parent.provider.id);
}

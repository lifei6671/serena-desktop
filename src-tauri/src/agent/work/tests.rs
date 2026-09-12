use super::*;

#[path = "terminal_tests.rs"]
mod terminal_tests;

fn workspace(id: &str, root: &str) -> Option<WorkspaceSnapshot> {
    Some(WorkspaceSnapshot {
        id: id.into(),
        root: root.into(),
    })
}

fn begin(workspace_id: &str, title: &str, goal: Option<&str>) -> UpdateAction {
    UpdateAction::Begin {
        workspace_id: workspace_id.into(),
        title: title.into(),
        goal: goal.map(str::to_owned),
    }
}

#[test]
fn begin_returns_exact_persisted_record_and_get_survives_service_and_store_reopen() {
    let dir = tempfile::tempdir().unwrap();
    tauri::async_runtime::block_on(async {
        let store = StateStore::open(dir.path().to_path_buf()).await.unwrap();
        let service = WorkProductService::new(store.clone());
        let before = now();
        let row = service
            .update(
                begin("workspace", "  修复任务  \n", Some(" goal ' ; -- ")),
                workspace("workspace", "C:/当前工作区/root"),
            )
            .await
            .unwrap();
        assert!(row.id.starts_with("work-"));
        assert_eq!(row.workspace_id, "workspace");
        assert_eq!(row.canonical_workspace_root, "C:/当前工作区/root");
        assert_eq!(row.title, "修复任务");
        assert_eq!(row.goal.as_deref(), Some(" goal ' ; -- "));
        assert_eq!(row.status, "active");
        assert_eq!(row.revision, 0);
        assert_eq!(row.acceptance_json, None);
        assert_eq!(row.completed_at, None);
        assert!((before..=now()).contains(&row.created_at));
        assert_eq!(row.updated_at, row.created_at);
        assert_eq!(
            store.work_run(row.id.clone()).await.unwrap(),
            Some(row.clone())
        );
        drop(service);
        drop(store);

        let store = StateStore::open(dir.path().to_path_buf()).await.unwrap();
        let service = WorkProductService::new(store);
        assert_eq!(
            service
                .query(QueryAction::Get {
                    work_run_id: row.id.clone()
                })
                .await
                .unwrap(),
            QueryData::WorkRun(row.clone())
        );
        // A later snapshot supplies its own root even for the same workspace id.
        // Begin has no caller-provided root field and retains no old root cache.
        let second = service
            .update(
                begin("workspace", "第二个任务", None),
                workspace("workspace", "D:/new-root"),
            )
            .await
            .unwrap();
        assert_ne!(row.id, second.id);
        assert_eq!(second.goal, None);
        assert_eq!(second.canonical_workspace_root, "D:/new-root");
    });
}

#[test]
fn begin_requires_matching_current_workspace_and_nonempty_snapshot_root() {
    let dir = tempfile::tempdir().unwrap();
    tauri::async_runtime::block_on(async {
        let store = StateStore::open(dir.path().to_path_buf()).await.unwrap();
        let service = WorkProductService::new(store.clone());
        for current in [
            None,
            workspace("other", "root"),
            workspace("workspace", ""),
            workspace("workspace", " \t\n"),
        ] {
            assert_eq!(
                service
                    .update(begin("workspace", "title", None), current)
                    .await
                    .unwrap_err(),
                "WORKSPACE_CONTEXT_MISMATCH"
            );
            assert!(store.list_work_runs(None, 100).await.unwrap().is_empty());
        }
    });
}

#[test]
fn malformed_ids_and_blank_titles_fail_before_any_insertion() {
    let dir = tempfile::tempdir().unwrap();
    tauri::async_runtime::block_on(async {
        let store = StateStore::open(dir.path().to_path_buf()).await.unwrap();
        let service = WorkProductService::new(store.clone());
        for id in [
            "",
            " ",
            "\t\n",
            " leading",
            "trailing ",
            "with space",
            "control\0id",
        ] {
            assert_eq!(
                service
                    .update(begin(id, "title", None), workspace(id, "root"))
                    .await
                    .unwrap_err(),
                "WORK_INVALID_ARGUMENT"
            );
            assert_eq!(
                service
                    .query(QueryAction::Get {
                        work_run_id: id.into()
                    })
                    .await
                    .unwrap_err(),
                "WORK_INVALID_ARGUMENT"
            );
            assert_eq!(
                service
                    .query(QueryAction::List {
                        workspace_id: Some(id.into()),
                        limit: None
                    })
                    .await
                    .unwrap_err(),
                "WORK_INVALID_ARGUMENT"
            );
        }
        for title in ["", " \n\t", "　"] {
            assert_eq!(
                service
                    .update(
                        begin("workspace", title, None),
                        workspace("workspace", "root")
                    )
                    .await
                    .unwrap_err(),
                "WORK_INVALID_ARGUMENT"
            );
        }
        assert!(store.list_work_runs(None, 100).await.unwrap().is_empty());
    });
}

#[test]
fn get_missing_is_not_found_and_opaque_ids_are_matched_exactly() {
    let dir = tempfile::tempdir().unwrap();
    tauri::async_runtime::block_on(async {
        let store = StateStore::open(dir.path().to_path_buf()).await.unwrap();
        let service = WorkProductService::new(store.clone());
        store
            .create_work_run(
                "自定义'ID".into(),
                "workspace".into(),
                "root".into(),
                "title".into(),
                None,
                1,
            )
            .await
            .unwrap();
        let row = store.work_run("自定义'ID".into()).await.unwrap().unwrap();
        assert_eq!(
            service
                .query(QueryAction::Get {
                    work_run_id: row.id.clone()
                })
                .await
                .unwrap(),
            QueryData::WorkRun(row)
        );
        for id in ["missing", "自定义'id"] {
            assert_eq!(
                service
                    .query(QueryAction::Get {
                        work_run_id: id.into()
                    })
                    .await
                    .unwrap_err(),
                "WORK_NOT_FOUND"
            );
        }
    });
}

#[test]
fn list_filters_workspace_and_preserves_stable_order_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    tauri::async_runtime::block_on(async {
        let store = StateStore::open(dir.path().to_path_buf()).await.unwrap();
        for (id, workspace, time) in [
            ("b", "workspace", 20),
            ("a", "workspace", 20),
            ("newest", "other", 30),
            ("oldest", "workspace", 10),
        ] {
            store
                .create_work_run(
                    id.into(),
                    workspace.into(),
                    "root".into(),
                    "title".into(),
                    None,
                    time,
                )
                .await
                .unwrap();
        }
        let before = store.list_work_runs(None, 100).await.unwrap();
        assert_eq!(
            before.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["newest", "a", "b", "oldest"]
        );
        drop(store);
        for _ in 0..2 {
            let store = StateStore::open(dir.path().to_path_buf()).await.unwrap();
            let service = WorkProductService::new(store.clone());
            assert_eq!(
                service
                    .query(QueryAction::List {
                        workspace_id: None,
                        limit: None
                    })
                    .await
                    .unwrap(),
                QueryData::List {
                    work_runs: before.clone()
                }
            );
            assert_eq!(
                service
                    .query(QueryAction::List {
                        workspace_id: Some("workspace".into()),
                        limit: Some(2)
                    })
                    .await
                    .unwrap(),
                QueryData::List {
                    work_runs: before[1..3].to_vec()
                }
            );
            assert_eq!(
                service
                    .query(QueryAction::List {
                        workspace_id: Some("missing".into()),
                        limit: None
                    })
                    .await
                    .unwrap(),
                QueryData::List { work_runs: vec![] }
            );
            assert_eq!(store.list_work_runs(None, 100).await.unwrap(), before);
        }
    });
}

#[test]
fn list_defaults_to_twenty_and_validates_limits_instead_of_clamping() {
    let dir = tempfile::tempdir().unwrap();
    tauri::async_runtime::block_on(async {
        let store = StateStore::open(dir.path().to_path_buf()).await.unwrap();
        let service = WorkProductService::new(store.clone());
        for i in 0..101 {
            store
                .create_work_run(
                    format!("w{i:03}"),
                    "workspace".into(),
                    "root".into(),
                    "title".into(),
                    None,
                    i,
                )
                .await
                .unwrap();
        }
        let expected = store.list_work_runs(None, 100).await.unwrap();
        for (limit, count) in [(None, 20), (Some(1), 1), (Some(100), 100)] {
            assert_eq!(
                service
                    .query(QueryAction::List {
                        workspace_id: None,
                        limit
                    })
                    .await
                    .unwrap(),
                QueryData::List {
                    work_runs: expected[..count].to_vec()
                }
            );
        }
        for limit in [0, 101, u32::MAX] {
            assert_eq!(
                service
                    .query(QueryAction::List {
                        workspace_id: None,
                        limit: Some(limit)
                    })
                    .await
                    .unwrap_err(),
                "WORK_INVALID_ARGUMENT"
            );
        }
    });
}

#[test]
fn work_operations_preserve_existing_execution_claim_and_runtime_data() {
    let dir = tempfile::tempdir().unwrap();
    tauri::async_runtime::block_on(async {
        let store = StateStore::open(dir.path().to_path_buf()).await.unwrap();
        store
            .product_create_fresh(
                "existing-execution".into(),
                "agent".into(),
                "request".into(),
                "prompt".into(),
                "workspace".into(),
                workspace("workspace", "root"),
                1,
            )
            .await
            .unwrap();
        let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
        db.execute(
            "INSERT INTO runtime_instances (id,owner_host_instance_id,state,created_at,updated_at)
             VALUES ('existing-runtime','host','unknown',1,1)",
            [],
        )
        .unwrap();
        let snapshot = || {
            [
                "executions",
                "workspace_claims",
                "runtime_instances",
                "execution_runtime_attempts",
                "work_execution_links",
            ]
            .map(|table| {
                let mut statement = db.prepare(&format!("SELECT * FROM {table}")).unwrap();
                let columns = statement.column_count();
                statement
                    .query_map([], |row| {
                        (0..columns)
                            .map(|i| row.get::<_, rusqlite::types::Value>(i))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
            })
        };
        let before = snapshot();
        let service = WorkProductService::new(store.clone());
        // An existing Execution owns this root's Claim; Work must not acquire one.
        let row = service
            .update(
                begin("workspace", "  logical work  ", Some(" \t\0opaque goal\n")),
                workspace("workspace", "root"),
            )
            .await
            .unwrap();
        assert_eq!(row.title, "logical work");
        assert_eq!(row.goal.as_deref(), Some(" \t\0opaque goal\n"));
        assert_eq!(snapshot(), before);
        assert_eq!(
            service
                .query(QueryAction::Get {
                    work_run_id: row.id.clone()
                })
                .await
                .unwrap(),
            QueryData::WorkRun(row.clone())
        );
        assert_eq!(
            service
                .query(QueryAction::List {
                    workspace_id: None,
                    limit: None
                })
                .await
                .unwrap(),
            QueryData::List {
                work_runs: vec![row.clone()]
            }
        );
        for current in [
            None,
            workspace("other", "root"),
            workspace("workspace", " \t"),
        ] {
            assert_eq!(
                service
                    .update(begin("workspace", "rejected", None), current)
                    .await
                    .unwrap_err(),
                "WORKSPACE_CONTEXT_MISMATCH"
            );
            assert_eq!(
                store.list_work_runs(None, 100).await.unwrap(),
                vec![row.clone()]
            );
            assert_eq!(snapshot(), before);
        }
    });
}

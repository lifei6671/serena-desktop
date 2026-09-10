use super::*;
use crate::agent::execution::{CreateExecutionInput, ExecutionMode, canonicalize_request};

#[test]
fn legacy_read_only_is_readable_and_retryable_without_upgrade() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let request: CreateExecutionInput = serde_json::from_value(json!({
            "agent_id":"legacy", "request_key":"first", "prompt":"original",
            "execution_profile":{}, "workspace_id":"original",
            "canonical_workspace_root":temp.path(), "mode":"read_only"
        }))
        .unwrap();
        let canonical = canonicalize_request(request.clone()).unwrap();
        let old_hash = canonical.request_hash().to_owned();
        let mut write_request = request;
        write_request.mode = ExecutionMode::WorkspaceWrite;
        assert_ne!(
            old_hash,
            canonicalize_request(write_request).unwrap().request_hash()
        );
        store
            .create_execution("legacy-e".into(), canonical, 1)
            .await
            .unwrap();
        let before = store.execution("legacy-e".into()).await.unwrap().unwrap();
        let service = AgentProductService::new(store.clone());
        let retry = service.checked_operation(json!({"action":"start","agentId":"legacy","requestKey":"first","prompt":"original"}), None).await;
        assert_eq!(retry["data"]["executionId"], "legacy-e");
        for action in [
            json!({"action":"observe","waitMs":0,"executionId":"legacy-e"}),
            json!({"action":"list"}),
        ] {
            let result = service.checked_operation(action.clone(), None).await;
            assert_eq!(result["ok"], true);
            let view = if action["action"] == "list" {
                &result["data"]["executions"][0]
            } else {
                &result["data"]
            };
            assert_eq!(view["availableActions"]["canContinue"], false);
            assert_eq!(view["availableActions"]["canResumePending"], true);
        }
        assert_eq!(service.checked_operation(json!({"action":"continue","executionId":"legacy-e","requestKey":"next","prompt":"new"}),None).await["error"]["code"], "AGENT_CONTINUE_NOT_ALLOWED");
        assert_eq!(
            service
                .checked_operation(
                    json!({"action":"start","agentId":"legacy","requestKey":"next","prompt":"new"}),
                    None
                )
                .await["error"]["code"],
            "AGENT_LINEAGE_CONFLICT"
        );
        assert_eq!(
            store.execution("legacy-e".into()).await.unwrap().unwrap(),
            before
        );
        drop(service);
        drop(store);
        let reopened = StateStore::open(temp.path().join("store")).await.unwrap();
        assert_eq!(
            reopened
                .execution("legacy-e".into())
                .await
                .unwrap()
                .unwrap(),
            before
        );
        assert_eq!(
            reopened
                .workspace_claim(before.canonical_workspace_root)
                .await
                .unwrap()
                .unwrap()
                .execution_id,
            "legacy-e"
        );
    });
}

#[test]
fn product_permissions_are_not_caller_selectable() {
    for field in [
        "root",
        "cwd",
        "thread",
        "runtime",
        "provider",
        "mode",
        "sandbox",
        "profile",
        "readOnly",
        "workspaceWrite",
        "allowWrite",
        "approvalPolicy",
    ] {
        let mut request = start("a", "key");
        request[field] = json!("caller-selected");
        assert!(
            parse(request)
                .unwrap_err()
                .starts_with("AGENT_INVALID_ARGUMENT"),
            "{field}"
        );
    }
}

#[test]
#[ignore = "Isolated fixed Codex workspace-write Product Contract; run alone"]
fn real_fixed_workspace_write_product_contract() {
    // Outside TEMP: workspace-write may legitimately allow OS temporary roots.
    let profile = std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
    let temp = tempfile::Builder::new()
        .prefix("serena-write-contract-")
        .tempdir_in(&profile)
        .unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    assert!(
        std::process::Command::new("git")
            .arg("init")
            .arg(&workspace)
            .output()
            .unwrap()
            .status
            .success()
    );
    let workspace = workspace.canonicalize().unwrap();
    let outside = temp.path().join("outside-sentinel.txt");
    let script = format!(
        r#"param([string]$Stage)
$ErrorActionPreference = 'Stop'
[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'created.txt'), 'created-' + $Stage)
[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'existing.txt'), 'modified-' + $Stage)
$outsideDenied = $false
$outsideError = ''
try {{ [IO.File]::WriteAllText('{}', 'must-be-denied') }} catch {{
    $exception = $_.Exception
    while ($exception.InnerException) {{ $exception = $exception.InnerException }}
    $outsideError = $exception.GetType().FullName
    $outsideDenied = $outsideError -eq 'System.UnauthorizedAccessException'
}}
@{{stage=$Stage; commandResult=(6*7); outsideDenied=$outsideDenied; outsideError=$outsideError}} | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $PSScriptRoot ('receipt-' + $Stage + '.json'))
Write-Output ('WORKSPACE_WRITE_CONTRACT_' + $Stage)
"#,
        outside.to_str().unwrap().replace("'", "''")
    );
    std::fs::write(workspace.join("workspace-write-contract.ps1"), &script).unwrap();
    std::fs::write(workspace.join("existing.txt"), "original").unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(profile.join(".codex/auth.json"), home.join("auth.json")).unwrap();
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/tasks/evidence/TASK-009/workspace-write-amendment-2026-09-09")
        .join(format!("real-{}-{}", std::process::id(), now()));
    std::fs::create_dir_all(&evidence).unwrap();
    struct Env(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Env {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                unsafe {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }
    let _env = Env(vec![
        ("CODEX_HOME", std::env::var_os("CODEX_HOME")),
        (
            "SERENA_CONTRACT_RAW_DIR",
            std::env::var_os("SERENA_CONTRACT_RAW_DIR"),
        ),
    ]);
    // Invoked alone; mirrors the existing fixed-binary Contract test isolation.
    unsafe {
        std::env::set_var("CODEX_HOME", &home);
        std::env::set_var("SERENA_CONTRACT_RAW_DIR", &evidence);
    }
    println!("Workspace-write evidence: {}", evidence.display());
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let audit = rusqlite::Connection::open(temp.path().join("store/agent-state.db")).unwrap();
        audit.execute_batch("CREATE TABLE release_trace(execution_id TEXT,status TEXT,evidence TEXT); CREATE TRIGGER contract_release AFTER DELETE ON workspace_claims BEGIN INSERT INTO release_trace SELECT id,status,release_evidence_state FROM executions WHERE id=old.execution_id; END;").unwrap();
        let (service, recovery) = AgentProductService::initialize(store.clone())
            .await
            .unwrap();
        assert!(recovery.is_empty());
        let mut prior: Option<crate::agent::store::ExecutionRecord> = None;
        for stage in ["start", "continue"] {
            let prompt = format!(
                "Run exactly this controlled local integration test command in the current workspace using your shell tool: powershell.exe -NoProfile -NonInteractive -File ./workspace-write-contract.ps1 -Stage {stage}. The script creates/modifies only disposable test files and checks the sandbox denies a disposable outside sentinel. Do not edit the script, emulate its output, request elevated permissions, or bypass the sandbox. Then report the actual command output."
            );
            let request = match &prior {
                None => {
                    json!({"action":"start","agentId":"write-contract","requestKey":stage,"prompt":prompt})
                }
                Some(row) => {
                    json!({"action":"continue","executionId":row.id,"requestKey":stage,"prompt":prompt})
                }
            };
            let response = service
                .checked_operation(
                    request.clone(),
                    if prior.is_none() {
                        w(&workspace, "write-workspace")
                    } else {
                        None
                    },
                )
                .await;
            std::fs::write(
                evidence.join(format!("{stage}-receipt.json")),
                response.to_string(),
            )
            .unwrap();
            assert_eq!(response["ok"], true, "{response}");
            let id = response["data"]["executionId"].as_str().unwrap();
            let view = final_row(&service, id).await;
            let row = store.execution(id.into()).await.unwrap().unwrap();
            let runtime_id = row.runtime_instance_id.clone().unwrap();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
            let runtime = loop {
                let runtime = store.runtime(runtime_id.clone()).await.unwrap().unwrap();
                if runtime.termination_evidence_state == "complete" {
                    break runtime;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "Runtime termination evidence missing"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            };
            let stdin =
                std::fs::read_to_string(evidence.join(format!("{runtime_id}.stdin.raw.jsonl")))
                    .unwrap();
            let stdout =
                std::fs::read_to_string(evidence.join(format!("{runtime_id}.stdout.raw.jsonl")))
                    .unwrap();
            let requests: Vec<Value> = stdin
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            let messages: Vec<Value> = stdout
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            let method = if prior.is_none() {
                "thread/start"
            } else {
                "thread/resume"
            };
            let request = requests.iter().find(|r| r["method"] == method).unwrap();
            let actual = &messages
                .iter()
                .find(|r| r.get("id") == request.get("id") && r.get("result").is_some())
                .unwrap()["result"];
            let receipt_path = workspace.join(format!("receipt-{stage}.json"));
            let receipt = std::fs::read_to_string(&receipt_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(s.trim_start_matches('\u{feff}')).ok());
            let outside_exists = outside.exists();
            std::fs::write(evidence.join(format!("{stage}-evidence.json")), serde_json::to_vec_pretty(&json!({"view":view,"mode":row.mode,"sandbox":actual["sandbox"],"cwd":actual["cwd"],"runtimeState":runtime.state,"terminationEvidence":runtime.termination_evidence_state,"resultCompleteness":row.result_completeness,"cleanup":row.background_cleanup_state,"release":row.release_evidence_state,"commandReceipt":receipt,"outsideExists":outside_exists})).unwrap()).unwrap();
            assert_eq!(
                actual["sandbox"]["type"], "workspaceWrite",
                "Material Contract Difference: actual sandbox"
            );
            assert_eq!(
                std::path::Path::new(actual["cwd"].as_str().unwrap())
                    .canonicalize()
                    .unwrap(),
                workspace
            );
            assert_eq!(row.mode, "workspace_write");
            assert_eq!(row.status, "completed");
            assert_eq!(row.provider_terminal_status.as_deref(), Some("completed"));
            assert_eq!(row.background_cleanup_state, "empty");
            assert_eq!(row.result_completeness, "complete");
            assert_eq!(row.release_evidence_state, "complete");
            assert!(
                store
                    .workspace_claim(row.canonical_workspace_root.clone())
                    .await
                    .unwrap()
                    .is_none()
            );
            let release: (String, String) = audit
                .query_row(
                    "SELECT status,evidence FROM release_trace WHERE execution_id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(release, ("completed".into(), "complete".into()));
            assert_eq!(
                std::fs::read_to_string(workspace.join("workspace-write-contract.ps1")).unwrap(),
                script
            );
            assert_eq!(
                std::fs::read_to_string(workspace.join("created.txt")).unwrap(),
                format!("created-{stage}")
            );
            assert_eq!(
                std::fs::read_to_string(workspace.join("existing.txt")).unwrap(),
                format!("modified-{stage}")
            );
            let receipt = receipt.expect("Controlled command did not produce its receipt");
            assert_eq!(receipt["commandResult"], 42);
            assert_eq!(
                receipt["outsideDenied"], true,
                "Material Contract Difference: {receipt}"
            );
            assert!(
                !outside_exists,
                "Material Contract Difference: workspace-external write succeeded"
            );
            let retry = service.checked_operation(if stage == "start" { json!({"action":"start","agentId":"write-contract","requestKey":stage,"prompt":prompt}) } else { json!({"action":"continue","executionId":prior.as_ref().unwrap().id,"requestKey":stage,"prompt":prompt}) }, None).await;
            assert_eq!(retry["data"]["executionId"], id);
            assert_eq!(
                requests
                    .iter()
                    .filter(|r| r["method"] == "turn/start")
                    .count(),
                1
            );
            if let Some(previous) = prior {
                assert_eq!(previous.thread_id, row.thread_id);
                assert_ne!(previous.turn_id, row.turn_id);
                assert_ne!(previous.runtime_instance_id, row.runtime_instance_id);
            }
            prior = Some(row);
        }
    });
}

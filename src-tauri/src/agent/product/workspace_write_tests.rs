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

#[cfg(windows)]
#[test]
#[ignore = "Explicit fixed Codex read-only network + filesystem sandbox smoke; run alone"]
fn real_fixed_read_only_network_sandbox_contract() {
    // Nest a disposable Git Workspace in the repository so Windows can launch the
    // restricted process while the sibling sentinel remains outside that Workspace.
    let profile = std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
    let temp = tempfile::Builder::new()
        .prefix("serena-read-network-contract-")
        .tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap(),
        )
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
    let inside = workspace.join("read-only-must-not-write.txt");
    let outside = temp.path().join("outside-must-not-write.txt");
    let home = temp.path().join("home");
    let raw = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/tasks/evidence/TASK-005/codex-0.153.4/network-smoke-2026-09-11")
        .join(format!("read-only-{}-{}", std::process::id(), now()));
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir_all(&raw).unwrap();
    println!("Read-only network evidence: {}", raw.display());
    std::fs::copy(profile.join(".codex/auth.json"), home.join("auth.json")).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "[windows]\nsandbox = 'elevated'\n",
    )
    .unwrap();
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
    unsafe {
        std::env::set_var("CODEX_HOME", &home);
        std::env::set_var("SERENA_CONTRACT_RAW_DIR", &raw);
    }
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let manager = AgentTaskManager::new(
            store,
            profile.join(r"AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe"),
        );
        let prompt = format!(
            "First run exactly this read-only network command: python -c \"import urllib.request; r=urllib.request.urlopen('https://example.com/', timeout=10); print('NETWORK_STATUS='+str(r.status))\". Then attempt to create ./read-only-must-not-write.txt and '{}' using PowerShell; both writes must remain denied. Do not request approval, bypass the sandbox, or claim success without the real network output. Report the actual HTTP status and write denials.",
            outside.to_str().unwrap().replace("'", "''")
        );
        let request: CreateExecutionInput = serde_json::from_value(json!({
            "agent_id":"read-network-contract",
            "request_key":"read-network-contract",
            "prompt":prompt,
            "execution_profile":{},
            "workspace_id":"read-network-workspace",
            "canonical_workspace_root":workspace,
            "mode":"read_only"
        }))
        .unwrap();
        let execution = tokio::time::timeout(Duration::from_secs(180), manager.execute(request))
            .await
            .expect("read-only network smoke deadline")
            .unwrap()
            .execution;
        assert_eq!(execution.status, "completed");
        assert_eq!(
            execution.provider_terminal_status.as_deref(),
            Some("completed")
        );
        assert_eq!(execution.release_evidence_state, "complete");
        assert!(!inside.exists(), "read-only sandbox wrote inside Workspace");
        assert!(
            !outside.exists(),
            "read-only sandbox wrote outside Workspace"
        );
        let runtime = execution.runtime_instance_id.unwrap();
        let stdin =
            std::fs::read_to_string(raw.join(format!("{runtime}.stdin.raw.jsonl"))).unwrap();
        let stdout =
            std::fs::read_to_string(raw.join(format!("{runtime}.stdout.raw.jsonl"))).unwrap();
        let requests: Vec<Value> = stdin
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let turn = requests
            .iter()
            .find(|request| request["method"] == "turn/start")
            .unwrap();
        assert_eq!(turn["params"]["approvalPolicy"], "never");
        assert_eq!(
            turn["params"]["sandboxPolicy"],
            json!({"type":"readOnly","networkAccess":true})
        );
        assert!(!stdin.contains("dangerFullAccess"));
        let messages: Vec<Value> = stdout
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let effective_settings = messages
            .iter()
            .find(|message| message["method"] == "thread/settings/updated")
            .expect("Codex did not confirm effective Turn settings");
        assert_eq!(
            effective_settings["params"]["threadSettings"]["sandboxPolicy"],
            json!({"type":"readOnly","networkAccess":true})
        );
        assert_eq!(
            std::path::Path::new(
                effective_settings["params"]["threadSettings"]["cwd"]
                    .as_str()
                    .unwrap()
            )
            .canonicalize()
            .unwrap(),
            workspace
        );
        assert!(!messages.iter().any(|message| {
            message["method"] == "item/permissions/requestApproval"
                || message["method"] == "item/commandExecution/requestApproval"
                || message["method"] == "item/fileChange/requestApproval"
        }));
        let command_outputs: Vec<_> = messages
            .iter()
            .filter_map(|message| message.pointer("/params/item"))
            .filter(|item| item["type"] == "commandExecution")
            .cloned()
            .collect();
        let command_output = command_outputs
            .iter()
            .filter_map(|item| item["aggregatedOutput"].as_str())
            .find(|output| output.contains("NETWORK_STATUS=200"))
            .unwrap_or_else(|| {
                panic!(
                    "sandboxed HTTPS response missing; evidence={}",
                    raw.display()
                )
            });
        assert!(command_output.contains("200"), "{command_output}");
        assert!(
            command_outputs
                .iter()
                .filter_map(|item| item["aggregatedOutput"].as_str())
                .filter(|output| output.contains("denied"))
                .count()
                >= 2,
            "read-only write denials missing; evidence={}",
            raw.display()
        );
        std::fs::write(
            raw.join("evidence.json"),
            serde_json::to_vec_pretty(&json!({
                "turnSandboxPolicy":turn["params"]["sandboxPolicy"],
                "effectiveTurnSettings":effective_settings["params"]["threadSettings"],
                "commandOutputs":command_outputs,
                "insideExists":inside.exists(),
                "outsideExists":outside.exists()
            }))
            .unwrap(),
        )
        .unwrap();
    });
}

#[cfg(windows)]
#[test]
#[ignore = "Isolated fixed Codex workspace-write + network Product Contract; run alone"]
fn real_fixed_workspace_write_network_product_contract() {
    // Nest a disposable Git Workspace in the repository so Windows can launch the
    // restricted process while the sibling sentinel remains outside that Workspace.
    let profile = std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
    let temp = tempfile::Builder::new()
        .prefix("serena-write-contract-")
        .tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap(),
        )
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
    std::fs::write(workspace.join("existing.txt"), "original").unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(profile.join(".codex/auth.json"), home.join("auth.json")).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "[windows]\nsandbox = 'elevated'\n",
    )
    .unwrap();
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
                "Use the shell tool to run these three Python standard-library commands separately and in order. (1) `python -c \"import urllib.request; r=urllib.request.urlopen('https://example.com/', timeout=10); print('NETWORK_STATUS='+str(r.status))\"`. (2) `python -c \"from pathlib import Path; Path('created.txt').write_text('created-{stage}'); Path('existing.txt').write_text('modified-{stage}'); print('WORKSPACE_WRITE_OK')\"`. (3) `python -c \"from pathlib import Path; Path(r'{}').write_text('must-be-denied')\"`. The first command must make the real HTTPS request from inside the Codex sandbox, and the third command must remain denied. Do not request approval, bypass the sandbox, emulate output, or use another command. Report the actual outputs.",
                outside.to_str().unwrap().replace("'", "''")
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
            let turn_deadline = tokio::time::Instant::now() + Duration::from_secs(240);
            let view = loop {
                let view = service.observe(id.into(), true).await.unwrap();
                if matches!(
                    view.status.as_str(),
                    "completed" | "failed" | "cancelled" | "interrupted"
                ) {
                    break view;
                }
                assert!(
                    !matches!(view.status.as_str(), "unknown" | "reconciling"),
                    "unexpected failure: {view:?}"
                );
                assert!(
                    tokio::time::Instant::now() < turn_deadline,
                    "network contract deadline: {view:?}"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            };
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
            let turn_request = requests
                .iter()
                .find(|request| request["method"] == "turn/start")
                .unwrap();
            assert_eq!(turn_request["params"]["approvalPolicy"], "never");
            assert_eq!(
                turn_request["params"]["sandboxPolicy"],
                json!({
                    "type":"workspaceWrite",
                    "writableRoots":[row.canonical_workspace_root.clone()],
                    "networkAccess":true,
                    "excludeTmpdirEnvVar":false,
                    "excludeSlashTmp":false
                })
            );
            let messages: Vec<Value> = stdout
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            let effective_settings = messages
                .iter()
                .find(|message| message["method"] == "thread/settings/updated")
                .expect("Codex did not confirm effective Turn settings");
            let effective_policy = &effective_settings["params"]["threadSettings"]["sandboxPolicy"];
            assert_eq!(effective_policy["type"], "workspaceWrite");
            assert_eq!(effective_policy["networkAccess"], true);
            assert_eq!(effective_policy["excludeTmpdirEnvVar"], false);
            assert_eq!(effective_policy["excludeSlashTmp"], false);
            // Codex reports only additional roots here and normalizes away the
            // explicit root because it equals the inherited Thread cwd.
            assert_eq!(effective_policy["writableRoots"], json!([]));
            assert_eq!(
                std::path::Path::new(
                    effective_settings["params"]["threadSettings"]["cwd"]
                        .as_str()
                        .unwrap()
                )
                .canonicalize()
                .unwrap(),
                workspace
            );
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
            let outside_exists = outside.exists();
            let command_outputs: Vec<_> = messages
                .iter()
                .filter_map(|message| message.pointer("/params/item"))
                .filter(|item| item["type"] == "commandExecution")
                .cloned()
                .collect();
            assert!(
                command_outputs.iter().any(|item| item["aggregatedOutput"]
                    .as_str()
                    .is_some_and(|output| output.contains("NETWORK_STATUS=200"))),
                "sandboxed HTTPS response missing; evidence={}",
                evidence.display()
            );
            assert!(command_outputs.iter().any(|item| {
                item["status"] == "failed"
                    && item["aggregatedOutput"]
                        .as_str()
                        .is_some_and(|output| output.contains("PermissionError"))
            }));
            assert!(!messages.iter().any(|message| {
                message["method"] == "item/permissions/requestApproval"
                    || message["method"] == "item/commandExecution/requestApproval"
                    || message["method"] == "item/fileChange/requestApproval"
            }));
            std::fs::write(evidence.join(format!("{stage}-evidence.json")), serde_json::to_vec_pretty(&json!({"view":view,"mode":row.mode,"threadStartOrResumeSandbox":actual["sandbox"],"turnSandboxPolicy":turn_request["params"]["sandboxPolicy"],"effectiveTurnSettings":effective_settings["params"]["threadSettings"],"cwd":actual["cwd"],"runtimeState":runtime.state,"terminationEvidence":runtime.termination_evidence_state,"resultCompleteness":row.result_completeness,"cleanup":row.background_cleanup_state,"release":row.release_evidence_state,"commandOutputs":command_outputs,"outsideExists":outside_exists})).unwrap()).unwrap();
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
                std::fs::read_to_string(workspace.join("created.txt")).unwrap(),
                format!("created-{stage}")
            );
            assert_eq!(
                std::fs::read_to_string(workspace.join("existing.txt")).unwrap(),
                format!("modified-{stage}")
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

#[cfg(windows)]
#[test]
#[ignore = "Explicit fixed Codex Agent Activity Product smoke; run alone"]
fn real_fixed_agent_activity_product_smoke() {
    let profile = std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
    let temp = tempfile::Builder::new()
        .prefix("serena-activity-contract-")
        .tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap(),
        )
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
    std::fs::create_dir(workspace.join("src")).unwrap();
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"activity-smoke\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("src/lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn slow_activity_probe() {\n        std::thread::sleep(std::time::Duration::from_secs(3));\n        assert_eq!(2 + 2, 4);\n    }\n}\n",
    )
    .unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(profile.join(".codex/auth.json"), home.join("auth.json")).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "[windows]\nsandbox = 'elevated'\n",
    )
    .unwrap();
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!(
            "agent-progress-smoke-{}-{}",
            std::process::id(),
            now()
        ));
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
    unsafe {
        std::env::set_var("CODEX_HOME", &home);
        std::env::set_var("SERENA_CONTRACT_RAW_DIR", &evidence);
    }
    println!("Agent Activity evidence: {}", evidence.display());
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let (service, recovery) = AgentProductService::initialize(store.clone())
            .await
            .unwrap();
        assert!(recovery.is_empty());
        let prompt = "Use the shell tool for exactly two separate commands, in order. First run `python -c \"import time; print('COMMAND_STARTED'); time.sleep(3); print('COMMAND_DONE')\"`. After it completes, run `cargo test --offline`. Do not edit any file. Report both real command results.";
        let receipt = service
            .checked_operation(
                json!({
                    "action":"start",
                    "agentId":"activity-contract",
                    "requestKey":"activity-contract",
                    "prompt":prompt
                }),
                w(&workspace, "activity-workspace"),
            )
            .await;
        assert_eq!(receipt["ok"], true, "{receipt}");
        let execution_id = receipt["data"]["executionId"].as_str().unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
        let mut observed_command = false;
        let mut observed_test = false;
        let terminal = loop {
            let view = service.observe(execution_id.into(), false).await.unwrap();
            if view
                .progress
                .activity_phase
                .as_ref()
                .is_some_and(|phase| phase == &crate::agent::activity::ActivityPhase::Tool)
            {
                observed_command |= view
                    .progress
                    .tool_category
                    .as_ref()
                    .is_some_and(|category| {
                        category == &crate::agent::activity::ToolCategory::Command
                    });
                observed_test |= view
                    .progress
                    .tool_category
                    .as_ref()
                    .is_some_and(|category| {
                        category == &crate::agent::activity::ToolCategory::Test
                    });
            }
            if matches!(
                view.status.as_str(),
                "completed" | "failed" | "cancelled" | "interrupted"
            ) {
                break view;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Agent Activity smoke deadline: {view:?}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        assert_eq!(terminal.status, "completed", "{terminal:?}");
        assert_eq!(terminal.progress.phase, ProgressPhase::Terminal);
        assert!(
            observed_command,
            "ordinary command Activity was not observed"
        );
        assert!(observed_test, "test Activity was not observed");
        let row = store.execution(execution_id.into()).await.unwrap().unwrap();
        let runtime_id = row.runtime_instance_id.unwrap();
        let stdin = std::fs::read_to_string(evidence.join(format!("{runtime_id}.stdin.raw.jsonl")))
            .unwrap();
        let requests: Vec<Value> = stdin
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let turn = requests
            .iter()
            .find(|request| request["method"] == "turn/start")
            .unwrap();
        assert_eq!(turn["params"]["approvalPolicy"], "never");
        assert_eq!(turn["params"]["sandboxPolicy"]["type"], "workspaceWrite");
        assert_eq!(turn["params"]["sandboxPolicy"]["networkAccess"], true);
        assert!(!stdin.contains("dangerFullAccess"));
        std::fs::write(
            evidence.join("evidence.json"),
            serde_json::to_vec_pretty(&json!({
                "executionId":execution_id,
                "terminal":terminal,
                "observedCommand":observed_command,
                "observedTest":observed_test,
                "turnApprovalPolicy":turn["params"]["approvalPolicy"],
                "turnSandboxPolicy":turn["params"]["sandboxPolicy"]
            }))
            .unwrap(),
        )
        .unwrap();
    });
}

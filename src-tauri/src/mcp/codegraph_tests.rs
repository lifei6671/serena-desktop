use super::*;
use std::path::Path;

fn workspace(root: &Path) -> Workspace {
    Workspace {
        id: "fixture".into(),
        name: "Fixture".into(),
        root: root.canonicalize().unwrap(),
    }
}
fn logs() -> Logs {
    Arc::new(Mutex::new(VecDeque::new()))
}
pub(crate) fn release_signal(binding: &Binding) -> CancellationToken {
    binding.cancel.clone()
}
pub(crate) fn fixture(workspace: &Workspace, generation: u64, mode: &str, logs: Logs) -> Binding {
    let root = &workspace.root;
    std::fs::create_dir_all(root.join(".codegraph")).unwrap();
    std::fs::write(root.join(".codegraph/codegraph.db"), b"SQLite format 3\0").unwrap();
    std::fs::write(root.join("mode"), mode).unwrap();
    let script = root.join("graph-fixture.cjs");
    std::fs::write(&script, r#"
const fs = require('node:fs'), readline = require('node:readline');
const count = Number(fs.existsSync('starts') ? fs.readFileSync('starts','utf8') : 0) + 1;
fs.writeFileSync('starts', String(count));
console.error('fixture diagnostic');
const mode = fs.readFileSync('mode','utf8');
readline.createInterface({input:process.stdin}).on('line', line => {
 const m=JSON.parse(line); if (!('id' in m)) return;
 fs.appendFileSync('methods',m.method+'\n');
 if (m.method === 'initialize' && mode === 'start-fail') process.exit(8);
 let result;
 if (m.method === 'initialize') result={protocolVersion:m.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'fixture',version:'1'}};
 if (m.method === 'tools/list') {
   const schema={type:'object',properties:{query:{type:'string'},maxFiles:{type:'number'}},required:['query']};
   if(mode==='missing-query') delete schema.properties.query;
   if(mode==='missing-maxFiles') delete schema.properties.maxFiles;
   if(mode==='query-optional') schema.required=[];
   if(mode==='missing-required') delete schema.required;
   if(mode==='maxFiles-required') schema.required=['query','maxFiles'];
   if(mode==='unknown-required') schema.required=['query','projectPath'];
   result={tools:[{name:mode==='bad-schema'?'other':'codegraph_explore',inputSchema:schema}]};
 }
 if (m.method === 'tools/call') {
   const a=m.params.arguments;
   fs.appendFileSync('requests',JSON.stringify(a)+'\n');
   if(a.query==='crash' || a.query==='crash-once' && count===1) process.exit(7);
   if(a.query==='hang') return;
   if(a.query==='exit-idle') setTimeout(()=>process.exit(7),30);
   result={content:[{type:'text',text:JSON.stringify({root:process.cwd(),args:a,count})}],isError:a.query==='tool-error'};
 }
 const send=()=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,result})+'\n');
 if(m.method==='initialize' && mode==='slow') setTimeout(send,200); else send();
});
process.stdin.on('end',()=>process.exit(0));
"#).unwrap();
    let mut binding = Binding::new(workspace, generation, logs);
    Arc::get_mut(&mut binding.runtime).unwrap().mock = Some(script);
    binding.start();
    binding
}
pub(super) async fn settled(binding: &Binding, workspace: &Workspace, generation: u64) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = binding.status(workspace, generation);
            if status["status"] != "starting" {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
pub(crate) async fn mock(workspace: &Workspace, generation: u64, logs: Logs) -> Binding {
    let binding = fixture(workspace, generation, "ready", logs);
    assert_eq!(
        settled(&binding, workspace, generation).await["status"],
        "ready"
    );
    binding
}
fn starts(root: &Path) -> u32 {
    std::fs::read_to_string(root.join("starts"))
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn ready_never_queries_and_real_queries_freeze_max_files() {
    let dir = tempfile::tempdir().unwrap();
    let w = workspace(dir.path());
    let binding = mock(&w, 1, logs()).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("methods")).unwrap(),
        "initialize\ntools/list\n"
    );
    assert!(!dir.path().join("requests").exists());
    for (args, expected, count) in [
        (json!({"query":"x"}), 12, 1),
        (json!({"query":"x","maxFiles":3}), 3, 2),
    ] {
        binding
            .explore(&w, 1, args, CancellationToken::new())
            .await
            .unwrap();
        let requests = std::fs::read_to_string(dir.path().join("requests")).unwrap();
        let requests: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(requests.len(), count);
        assert_eq!(
            requests.last().unwrap(),
            &json!({"query":"x","maxFiles":expected})
        );
    }
}

#[tokio::test]
async fn incompatible_upstream_contracts_fail_without_queries() {
    for mode in [
        "missing-query",
        "missing-maxFiles",
        "query-optional",
        "missing-required",
        "maxFiles-required",
        "unknown-required",
        "bad-schema",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let w = workspace(dir.path());
        let log = logs();
        let binding = fixture(&w, 1, mode, log.clone());
        let status = settled(&binding, &w, 1).await;
        assert_eq!(status["status"], "start_failed", "{mode}: {status}");
        assert!(
            log.lock()
                .unwrap()
                .iter()
                .any(|entry| entry.contains("tool contract failure")),
            "{mode}"
        );
        assert!(!dir.path().join("requests").exists());
    }
}

#[tokio::test]
async fn missing_and_invalid_index_never_initialize() {
    let dir = tempfile::tempdir().unwrap();
    let w = workspace(dir.path());
    let binding = Binding::begin(&w, 1, logs());
    assert_eq!(
        binding
            .explore(&w, 1, json!({"query":"x"}), CancellationToken::new())
            .await,
        Err(Error::NotInitialized)
    );
    assert!(!dir.path().join(".codegraph").exists());
    std::fs::create_dir(dir.path().join(".codegraph")).unwrap();
    let empty = Binding::begin(&w, 2, logs());
    assert_eq!(empty.status(&w, 2)["status"], "not_initialized");
    std::fs::write(dir.path().join(".codegraph/codegraph.db"), b"broken").unwrap();
    let invalid = Binding::begin(&w, 3, logs());
    assert_eq!(invalid.status(&w, 3)["status"], "start_failed");
    assert_eq!(
        std::fs::read(dir.path().join(".codegraph/codegraph.db")).unwrap(),
        b"broken"
    );
}

#[tokio::test]
async fn id_root_and_generation_mismatch_never_dispatch() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let w = workspace(a.path());
    let binding = mock(&w, 41, logs()).await;
    let before = std::fs::read(a.path().join("requests")).unwrap_or_default();
    for (wrong, generation) in [
        (
            Workspace {
                id: "other".into(),
                ..w.clone()
            },
            41,
        ),
        (
            Workspace {
                root: b.path().canonicalize().unwrap(),
                ..w.clone()
            },
            41,
        ),
        (w.clone(), 42),
    ] {
        assert_eq!(
            binding
                .explore(
                    &wrong,
                    generation,
                    json!({"query":"crash"}),
                    CancellationToken::new()
                )
                .await,
            Err(Error::Unavailable)
        );
    }
    assert_eq!(
        std::fs::read(a.path().join("requests")).unwrap_or_default(),
        before
    );
    assert!(
        binding
            .explore(&w, 41, json!({"query":"x"}), CancellationToken::new())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn slow_old_generation_cannot_replace_new_binding_and_a_b_a_routes_correctly() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let wa = workspace(a.path());
    let wb = Workspace {
        id: "b".into(),
        ..workspace(b.path())
    };
    let log = logs();
    let old = fixture(&wa, 41, "slow", log.clone());
    assert_eq!(old.status(&wa, 41)["status"], "starting");
    assert_eq!(
        old.explore(&wa, 41, json!({"query":"x"}), CancellationToken::new())
            .await,
        Err(Error::Starting)
    );
    let next = mock(&wb, 42, log.clone()).await;
    settled(&old, &wa, 41).await; // A finishes after B; it owns no reference to B's slot.
    assert_eq!(next.status(&wb, 42)["generation"], 42);
    for (binding, w, generation) in [(&old, &wa, 41), (&next, &wb, 42)] {
        let text = binding
            .explore(
                w,
                generation,
                json!({"query":"x"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            Path::new(value["root"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            w.root
        );
        assert!(value["args"].get("projectPath").is_none());
    }
    drop(old);
    let again = mock(&wa, 43, log.clone()).await;
    drop(next);
    assert!(
        again
            .explore(&wa, 43, json!({"query":"x"}), CancellationToken::new())
            .await
            .is_ok()
    );
    let retired = fixture(&wb, 44, "slow", log.clone());
    drop(retired);
    assert_eq!(again.status(&wa, 43)["generation"], 43);
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|v| v.contains("generation discarded"))
    );
}

#[tokio::test]
async fn crash_recovers_once_and_repeated_failures_cool_down() {
    let a = tempfile::tempdir().unwrap();
    let wa = workspace(a.path());
    let log = logs();
    let binding = mock(&wa, 1, log.clone()).await;
    let text = binding
        .explore(
            &wa,
            1,
            json!({"query":"crash-once"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["count"], 2);
    assert_eq!(starts(a.path()), 2);
    let result = binding
        .explore(&wa, 1, json!({"query":"crash"}), CancellationToken::new())
        .await;
    assert_eq!(result, Err(Error::RuntimeLost));
    assert_eq!(starts(a.path()), 2);
    assert_eq!(binding.status(&wa, 1)["status"], "runtime_lost");
    binding.runtime.state.lock().unwrap().last_recovery = Some(Instant::now() - COOLDOWN);
    assert_eq!(
        binding
            .explore(&wa, 1, json!({"query":"crash"}), CancellationToken::new())
            .await,
        Err(Error::RuntimeLost)
    );
    assert_eq!(starts(a.path()), 3);
    for _ in 0..3 {
        assert_eq!(
            binding
                .explore(&wa, 1, json!({"query":"x"}), CancellationToken::new())
                .await,
            Err(Error::RuntimeLost)
        );
    }
    assert_eq!(starts(a.path()), 3);
    let entries = log
        .lock()
        .unwrap()
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(entries.contains("runtime recovery"));
    assert!(entries.contains("transport failure"));
}

#[tokio::test]
async fn start_failures_cool_down_and_tool_errors_do_not_restart() {
    let dir = tempfile::tempdir().unwrap();
    let w = workspace(dir.path());
    let binding = fixture(&w, 1, "start-fail", logs());
    assert_eq!(settled(&binding, &w, 1).await["status"], "start_failed");
    for _ in 0..3 {
        assert_eq!(
            binding
                .explore(&w, 1, json!({"query":"x"}), CancellationToken::new())
                .await,
            Err(Error::StartFailed)
        );
    }
    assert_eq!(starts(dir.path()), 2);
    drop(binding);
    let binding = mock(&w, 2, logs()).await;
    let count = starts(dir.path());
    assert_eq!(
        binding
            .explore(
                &w,
                2,
                json!({"query":"tool-error"}),
                CancellationToken::new()
            )
            .await,
        Err(Error::Upstream)
    );
    assert_eq!(binding.status(&w, 2)["status"], "ready");
    assert_eq!(starts(dir.path()), count);
}

#[tokio::test]
async fn concurrent_queries_share_one_recovery_after_idle_exit() {
    let dir = tempfile::tempdir().unwrap();
    let w = workspace(dir.path());
    let binding = mock(&w, 1, logs()).await;
    binding
        .explore(
            &w,
            1,
            json!({"query":"exit-idle"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while binding.runtime.current_error().is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let a = binding.explore(&w, 1, json!({"query":"a"}), CancellationToken::new());
    let b = binding.explore(&w, 1, json!({"query":"b"}), CancellationToken::new());
    let (a, b) = tokio::join!(a, b);
    assert!(a.is_ok());
    // A caller observing the in-flight recovery gets the stable Starting error.
    assert!(b.is_ok() || b == Err(Error::Starting));
    assert_eq!(starts(dir.path()), 2);
}

#[tokio::test]
async fn cancellation_and_dropped_query_invalidate_session_without_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let w = workspace(dir.path());
    let binding = mock(&w, 1, logs()).await;
    let cancel = CancellationToken::new();
    let result = binding.explore(&w, 1, json!({"query":"hang"}), cancel.clone());
    let cancelling = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancel.cancel();
    };
    assert_eq!(tokio::join!(result, cancelling).0, Err(Error::Cancelled));
    assert_eq!(binding.status(&w, 1)["status"], "runtime_lost");
    assert_eq!(starts(dir.path()), 1);
    drop(binding);
    let binding = mock(&w, 2, logs()).await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(30),
            binding.explore(&w, 2, json!({"query":"hang"}), CancellationToken::new())
        )
        .await
        .is_err()
    );
    assert_eq!(binding.status(&w, 2)["status"], "runtime_lost");
}

#[tokio::test]
#[ignore = "requires CODEGRAPH_TEST_PROJECT with an existing index; never initializes"]
async fn installed_cli_indexed_query() {
    let root = PathBuf::from(std::env::var_os("CODEGRAPH_TEST_PROJECT").unwrap())
        .canonicalize()
        .unwrap();
    let w = workspace(&root);
    let log = logs();
    let binding = Binding::begin(&w, 1, log.clone());
    let status = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let status = binding.status(&w, 1);
            if status["status"] != "starting" {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        status["status"],
        "ready",
        "{status}: {:?}",
        log.lock().unwrap()
    );
    let text = binding
        .explore(
            &w,
            1,
            json!({"query":"Broker activate","maxFiles":2}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(text.contains("Broker"), "{text}");
}

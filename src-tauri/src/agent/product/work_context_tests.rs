use super::*;

fn with_context(mut action: AgentExecuteAction, json: &str) -> AgentExecuteAction {
    match &mut action {
        AgentExecuteAction::Start {
            delegation_context_json,
            ..
        }
        | AgentExecuteAction::Continue {
            delegation_context_json,
            ..
        } => {
            *delegation_context_json = Some(json.into());
        }
        _ => unreachable!(),
    }
    action
}

fn reference(path: &str, bytes: &[u8]) -> String {
    json!({"summary":"Inspect these references", "files":[{
        "path":path, "sha256":Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>()
    }]})
    .to_string()
}

fn counts(root: &std::path::Path) -> Vec<i64> {
    let db = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
    [
        "executions",
        "workspace_claims",
        "work_execution_links",
        "runtime_instances",
        "execution_runtime_attempts",
    ]
    .iter()
    .map(|table| {
        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect()
}

#[tokio::test]
async fn known_raw_sha_and_empty_context_are_valid_without_source_copying() {
    use crate::agent::product::work_context::VersionedContext;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("abc"), b"abc").unwrap();
    let known = r#"{"files":[{"path":"abc","sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}]}"#;
    VersionedContext::parse(known)
        .unwrap()
        .verify(dir.path().to_string_lossy().into())
        .await
        .unwrap();
    for json in [r#"{"files":[]}"#, r#"{"summary":"Host summary"}"#] {
        let context = VersionedContext::parse(json).unwrap();
        let prompt = context.prompt("original task");
        assert!(prompt.ends_with("\nTask:\noriginal task"));
        context
            .verify("missing root is not read for empty refs".into())
            .await
            .unwrap();
    }
    assert!(VersionedContext::parse(r#"{"summary":null,"files":[]}"#).is_err());
}

#[tokio::test]
async fn versioned_context_is_frozen_before_dispatch_and_retries_ignore_file_drift() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = StateStore::open(root.into()).await.unwrap();
    create_work(&store, root, "work").await;
    let source = b"\xef\xbb\xbfPRIVATE_SOURCE_BODY\r\nnever copied into prompt\r\n";
    std::fs::write(root.join("source.txt"), source).unwrap();
    // Multiple chunks, raw non-UTF8 bytes, and stable ordering of references.
    let binary = vec![0xff; 150_001];
    std::fs::write(root.join("binary.bin"), &binary).unwrap();
    let sha = Sha256::digest(source)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let binary_sha = Sha256::digest(&binary)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let context = json!({"files":[{"sha256":sha,"path":"./source.txt"},{"path":"binary.bin","sha256":binary_sha}],"summary":"Inspect references"}).to_string();
    let request = with_context(start_work("work", "key"), &context);
    let (s, release, fake) = fake_service(
        store.clone(),
        root.join("agent-state.db"),
        "CONTEXT1",
        "T1",
        false,
        "paginated",
    )
    .await;
    let (one, two) = tokio::join!(
        s.agent_execute(request.clone(), w(root, "W")),
        s.agent_execute(request.clone(), w(root, "W"))
    );
    let one = one.unwrap();
    assert_eq!(two.unwrap().execution_id, one.execution_id);
    let id = one.execution_id;
    let link = store
        .work_execution_link(id.clone())
        .await
        .unwrap()
        .unwrap();
    let expected = format!(
        r#"{{"summary":"Inspect references","files":[{{"path":"binary.bin","sha256":"{binary_sha}"}},{{"path":"source.txt","sha256":"{sha}"}}]}}"#
    );
    assert_eq!(
        link.delegation_context_json.as_deref(),
        Some(expected.as_str())
    );
    let prompt = format!(
        "Host verified context:\nSummary: Inspect references\nVersioned source references:\n- binary.bin @ SHA256 {binary_sha}\n- source.txt @ SHA256 {sha}\n\nTask:\nhello"
    );
    assert_eq!(one.prompt, prompt);
    assert_eq!(
        store.execution(id.clone()).await.unwrap().unwrap().prompt,
        prompt
    );
    assert!(!prompt.contains("PRIVATE_SOURCE_BODY"));
    assert!(!prompt.contains(root.to_str().unwrap()));
    std::fs::write(root.join("source.txt"), b"changed after accepted").unwrap();
    for retry in [
        request.clone(),
        with_context(start_work("work", "key"), &expected),
    ] {
        assert_eq!(s.agent_execute(retry, None).await.unwrap().execution_id, id);
    }
    let mut changed_prompt = request.clone();
    if let AgentExecuteAction::Start { prompt, .. } = &mut changed_prompt {
        *prompt = "different task".into();
    }
    for conflict in [
        changed_prompt,
        with_context(
            start_work("work", "key"),
            &reference("missing.txt", b"missing"),
        ),
        with_context(start_work("work", "key"), "not JSON"),
        continue_work("work", &id, "key"),
    ] {
        assert_eq!(
            s.agent_execute(conflict, None).await.unwrap_err().code,
            "EXECUTION_REQUEST_KEY_CONFLICT"
        );
    }
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        1
    );
    release.send(()).unwrap();
    let terminal = final_row(&s, &id).await;
    assert_eq!(terminal.status, "completed");
    drop(s);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    let parent = store.execution(id.clone()).await.unwrap().unwrap();
    let before = counts(root);

    let (s, release, fake) = fake_service(
        store.clone(),
        root.join("agent-state.db"),
        "CONTEXT2",
        "T2",
        true,
        "paginated",
    )
    .await;
    // The previous refs are stale for a NEW continuation, before durable creation.
    assert_eq!(
        s.agent_execute(
            with_context(continue_work("work", &id, "next"), &context),
            None
        )
        .await
        .unwrap_err()
        .code,
        "CONTEXT_STALE"
    );
    assert_eq!(counts(root), before);
    let next_context = reference("source.txt", b"changed after accepted");
    let next_request = with_context(continue_work("work", &id, "next"), &next_context);
    let next = s.agent_execute(next_request.clone(), None).await.unwrap();
    assert_ne!(next.execution_id, id);
    assert_eq!(next.thread_id, terminal.thread_id);
    assert_eq!(
        store.execution(id.clone()).await.unwrap(),
        Some(parent.clone())
    );
    let next_link = store
        .work_execution_link(next.execution_id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next_link.parent_execution_id, Some(id.clone()));
    assert_eq!(
        next_link.delegation_context_json,
        Some(
            super::super::super::work_context::VersionedContext::parse(&next_context)
                .unwrap()
                .canonical_json()
        )
    );
    assert!(next.prompt.ends_with("\nTask:\nnext"));
    std::fs::remove_file(root.join("source.txt")).unwrap();
    assert_eq!(
        s.agent_execute(next_request, None)
            .await
            .unwrap()
            .execution_id,
        next.execution_id
    );
    assert_eq!(
        s.agent_execute(
            with_context(
                continue_work("work", &next.execution_id, "next"),
                &next_context
            ),
            None
        )
        .await
        .unwrap_err()
        .code,
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    release.send(()).unwrap();
    final_row(&s, &next.execution_id).await;
    assert_eq!(store.execution(id).await.unwrap(), Some(parent));
    drop(s);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    assert_eq!(methods.iter().filter(|m| *m == "thread/resume").count(), 1);
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        2
    );

    // Reopened state preserves frozen prompt/context, including inactive Work retries.
    let db = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
    db.execute("UPDATE work_runs SET status='completed'", [])
        .unwrap();
    drop(store);
    let reopened = StateStore::open(root.into()).await.unwrap();
    let s = AgentProductService::new(reopened.clone());
    let original = s.agent_execute(request, None).await.unwrap();
    assert_eq!(original.prompt, prompt);
    assert_eq!(
        reopened
            .work_execution_link(original.execution_id)
            .await
            .unwrap()
            .unwrap(),
        link
    );
}

#[tokio::test]
async fn invalid_and_stale_context_have_zero_durable_runtime_or_provider_effects() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = StateStore::open(root.into()).await.unwrap();
    create_work(&store, root, "work").await;
    let (s, _release, fake) = fake_service(
        store.clone(),
        root.join("agent-state.db"),
        "CONTEXT_REJECT",
        "T",
        false,
        "paginated",
    )
    .await;
    std::fs::write(root.join("source.txt"), b"abc").unwrap();
    let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let valid_ref = json!({"path":"source.txt", "sha256":sha});
    let mut invalid = vec![
        "not JSON".into(),
        json!({"summary":"  ","files":[]}).to_string(),
        json!({"files":[],"unknown":true}).to_string(),
        json!({"files":[{"path":"source.txt","sha256":sha,"unknown":true}]}).to_string(),
        json!({"files":[valid_ref.clone(),valid_ref]}).to_string(),
        json!({"files":[{"path":"source.txt","sha256":sha},{"path":"./source.txt","sha256":sha}]})
            .to_string(),
    ];
    for path in [
        "",
        " ",
        ".",
        "/absolute",
        "C:\\absolute",
        "C:relative",
        "../outside",
        "sub/../source.txt",
        "sub\\..\\source.txt",
        "source.txt:stream",
        "\\\\server\\share",
        "bad\npath",
    ] {
        invalid.push(json!({"files":[{"path":path,"sha256":sha}]}).to_string());
    }
    for sha in ["", "abc", &"A".repeat(64), &"g".repeat(64)] {
        invalid.push(json!({"files":[{"path":"source.txt","sha256":sha}]}).to_string());
    }
    let before = counts(root);
    assert_eq!(before, vec![0; 5]);
    for context in invalid {
        assert_eq!(
            s.agent_execute(
                with_context(start_work("work", "key"), &context),
                w(root, "W")
            )
            .await
            .unwrap_err()
            .code,
            "WORK_INVALID_ARGUMENT",
            "{context}"
        );
        assert_eq!(counts(root), before);
    }
    std::fs::create_dir(root.join("directory")).unwrap();
    let original = reference("source.txt", b"abc");
    std::fs::write(root.join("source.txt"), b"def").unwrap();
    for context in [
        original,
        reference("source.txt", b"wrong hash"),
        reference("missing", b""),
        reference("directory", b""),
    ] {
        let error = s
            .agent_execute(
                with_context(start_work("work", "key"), &context),
                w(root, "W"),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "CONTEXT_STALE");
        assert!(error.execution_id.is_none());
        assert_eq!(counts(root), before);
    }
    drop(s);
    assert!(fake.await.unwrap().is_empty());
}

#[cfg(windows)]
#[tokio::test]
async fn context_junction_outside_work_root_is_stale_before_creation() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("source.txt"), b"abc").unwrap();
    assert!(
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(dir.path().join("escape"))
            .arg(outside.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    let (s, _release, fake) = fake_service(
        store,
        dir.path().join("agent-state.db"),
        "CONTEXT_ESCAPE",
        "T",
        false,
        "paginated",
    )
    .await;
    let error = s
        .agent_execute(
            with_context(
                start_work("work", "key"),
                &reference("escape/source.txt", b"abc"),
            ),
            w(dir.path(), "W"),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "CONTEXT_STALE");
    assert_eq!(counts(dir.path()), vec![0; 5]);
    drop(s);
    assert!(fake.await.unwrap().is_empty());
}

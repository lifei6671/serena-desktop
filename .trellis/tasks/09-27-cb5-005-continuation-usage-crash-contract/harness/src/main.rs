//! task-local Stage A/B冻结路径与Stage C Crash探针；没有生产路径。
use agent_client_protocol::{
    ByteStreams, Client,
    schema::{ProtocolVersion, v1::*},
};
use futures::io::{AsyncRead, AsyncWrite};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::Path,
    pin::Pin,
    process::Stdio,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
mod transport;
use transport::*;
mod evidence;
use evidence::*;
mod runtime;
use runtime::*;
mod crash;
mod crash_runtime;
mod usage;
mod usage_runtime;
const META: &str = "codebuddy.ai/conversationRequestId";
const NODE: &str = r"C:\nvm4w\nodejs\node.exe";
const SCRIPT: &str =
    r"C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy";

/// 独立 scenario；只有完整 R1 terminal/reap 才能开始 R2，不 fallback。
async fn scenario(method: &str, launcher: &[String], timeout: Duration) -> io::Result<Value> {
    let cwd = tempfile::Builder::new()
        .prefix(&format!("cb5-005-{method}-"))
        .tempdir()?
        .keep();
    if cwd.to_string_lossy().starts_with(r"\\?\") {
        return Err(io::Error::other("VERBATIM_CWD_REJECTED"));
    }
    let token = uuid::Uuid::now_v7().to_string();
    let before = manifest(&cwd)?;
    let r1 = runtime(&cwd, None, &token, launcher, timeout).await;
    let middle = manifest(&cwd);
    let mut report = json!({"scenario":method,"status":"PARTIAL","cwd":cwd,"tokenSha256":hash(token.as_bytes()),"before":before,"windowsJobAtCreationProven":false,"claimReleaseInScope":false});
    if let Ok(r1) = r1 {
        let ready = r1["error"].is_null()
            && r1["terminalStopReason"] == "end_turn"
            && r1["terminalConversationIdMatched"] != false
            && r1["wrongSession"] == false
            && r1["cleanup"]["succeeded"] == true
            && middle.as_ref().is_ok_and(|m| m == &before);
        let sid = r1["sessionId"].as_str().unwrap_or("").to_owned();
        report["r1"] = r1;
        if ready {
            let r2 = runtime(&cwd, Some((method, &sid)), &token, launcher, timeout).await;
            if let Ok(r2) = r2 {
                let passed = r2["error"].is_null()
                    && r2["terminalStopReason"] == "end_turn"
                    && r2["terminalConversationIdMatched"] != false
                    && r2["matched"] == true
                    && r2["unattributedAnswerChunks"] == 0
                    && r2["wrongSession"] == false
                    && r2["cleanup"]["succeeded"] == true;
                report["status"] = json!(if passed {
                    "PASS"
                } else if r2["rpcErrorCode"] == -32601 && r2["cleanup"]["succeeded"] == true {
                    "UNSUPPORTED"
                } else {
                    "PARTIAL"
                });
                report["r2"] = r2;
            } else {
                report["r2Error"] = json!("RUNTIME_START_FAILED");
            }
        } else {
            report["r2Status"] = json!("NOT_RUN");
        }
    } else {
        report["r1Error"] = json!("RUNTIME_START_FAILED");
    }
    let after = manifest(&cwd);
    report["afterR1"] = json!(middle.as_ref().ok());
    report["afterR2"] = json!(after.as_ref().ok());
    report["workspaceDelta"] = json!(after.as_ref().ok().map(|m| {
        before
            .keys()
            .chain(m.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|k| before.get(*k) != m.get(*k))
            .collect::<Vec<_>>()
    }));
    report["manifestComplete"] = json!(middle.is_ok() && after.is_ok());
    if middle.is_err() || !after.as_ref().is_ok_and(|m| m == &before) {
        report["status"] = json!("PARTIAL");
    }
    // 只删除经过检查且为空的 root；异常文件/链接保留给 Host，绝不递归删除。
    report["workspaceDeleted"] =
        json!(after.is_ok_and(|m| m.is_empty()) && std::fs::remove_dir(&cwd).is_ok());
    if report["workspaceDeleted"] != true {
        report["status"] = json!("PARTIAL");
    }
    Ok(report)
}

/// 只接受独立方法，没有可重定向输出或重试的参数。
fn parse_scenario(args: &[String]) -> Option<&str> {
    (args.len() == 2).then(|| args[1].as_str()).filter(|m| {
        [
            "resume",
            "load",
            "usage",
            "usage-repair",
            "crash-before-terminal",
            "crash-after-terminal",
        ]
        .contains(m)
    })
}

/// 固定 task 路径独立于调用者 cwd。
fn evidence_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("evidence/continuation")
}

/// 固定编译时 task 输出路径：没有 output/force/retry 参数，换 cwd 不绕 sentinel。
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let Some(method) = parse_scenario(&args) else {
        eprintln!(
            "USAGE: cb5-005-continuation.exe resume|load|usage|usage-repair|crash-before-terminal|crash-after-terminal"
        );
        std::process::exit(2);
    };
    if method.starts_with("crash-") {
        std::process::exit(crash_runtime::host_run(method).await);
    }
    if method == "usage" {
        std::process::exit(usage_runtime::host_run().await);
    }
    if method == "usage-repair" {
        std::process::exit(usage_runtime::host_run_repair().await);
    }
    let root = evidence_root();
    let run=async {
        no_links(&root)?;
        durable(&root.join(format!("{method}.attempt-started.json")),&json!({"scenario":method,"replayAllowed":false,"attemptId":uuid::Uuid::now_v7().to_string()}))?;
        let launcher=vec![NODE.into(),SCRIPT.into(),"--acp".into()];
        let report=scenario(method,&launcher,Duration::from_secs(120)).await.unwrap_or_else(|_|json!({"scenario":method,"status":"PARTIAL","error":"SCENARIO_SETUP_FAILED"}));
        durable(&root.join(format!("{method}.result.json")),&report)?;
        println!("{}",json!({"scenario":method,"status":report["status"],"result":root.join(format!("{method}.result.json"))}));
        Ok::<_,io::Error>(report["status"]=="PASS")
    }.await;
    match run {
        Ok(true) => {}
        Ok(false) => std::process::exit(2),
        Err(_) => {
            eprintln!("SENTINEL_OR_EVIDENCE_FAILED_NO_RETRY");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod usage_tests;

#[cfg(test)]
mod crash_tests;

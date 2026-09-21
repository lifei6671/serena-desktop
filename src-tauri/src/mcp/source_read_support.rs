//! Phase 2B 本地 Source Read 共用的内部安全基础；公开 Tool 由各自任务接入。
#![allow(
    dead_code,
    reason = "P2B shared helpers are adopted incrementally by local Source handlers."
)]

use crate::{workspace_path::WorkspacePathResolver, workspace_resolver::WorkspaceLease};
use serde::Serialize;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

/// 通过唯一的通用 Workspace path authority 解析显式 Source `relative_path`。
pub(crate) fn resolve_relative_path(
    lease: &WorkspaceLease,
    relative_path: &str,
) -> Result<PathBuf, String> {
    WorkspacePathResolver::new(lease).resolve(relative_path)
}

/// 供无路径参数的 Source Tool 内部取得 Lease root，不接收 caller-provided path。
pub(crate) fn workspace_root(lease: &WorkspaceLease) -> Result<PathBuf, String> {
    WorkspacePathResolver::new(lease).root()
}

/// 仅由调用者给出的 hard byte budget 截断 UTF-8 文本，绝不切断字符。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundedText {
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

/// 在 UTF-8 字符边界内截断文本；具体 Tool 默认值由后续 Task 决定。
pub(crate) fn bounded_text(text: &str, hard_budget: usize) -> BoundedText {
    if text.len() <= hard_budget {
        return BoundedText {
            text: text.into(),
            truncated: false,
        };
    }
    let mut end = hard_budget;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    BoundedText {
        text: text[..end].into(),
        truncated: true,
    }
}

/// 统一的 Workspace provenance，不暴露任何 absolute root。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct WorkspaceProvenance {
    id: String,
    generation: u64,
}

/// 统一 Source 结果使用的 `{workspace:{id,generation}}` 外层 DTO。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct WorkspaceProvenanceEnvelope {
    workspace: WorkspaceProvenance,
}

/// 从已捕获 Lease 创建稳定 provenance；不读取当前 Registry 或 UI selection。
pub(crate) fn workspace_provenance(lease: &WorkspaceLease) -> WorkspaceProvenanceEnvelope {
    WorkspaceProvenanceEnvelope {
        workspace: WorkspaceProvenance {
            id: lease.workspace_id.clone(),
            generation: lease.generation,
        },
    }
}

/// 在 blocking 文件系统工作开始和每个遍历/读取循环检查 cancellation。
pub(crate) fn check_cancelled(cancel: &CancellationToken) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err("CANCELLED".into())
    } else {
        Ok(())
    }
}

/// 以同一个 Token 运行 blocking 工作；闭包必须在循环中调用 `check_cancelled`，不能忽略取消。
pub(crate) async fn run_blocking_cancellable<T, Work>(
    cancel: CancellationToken,
    work: Work,
) -> Result<T, String>
where
    T: Send + 'static,
    Work: FnOnce(CancellationToken) -> Result<T, String> + Send + 'static,
{
    check_cancelled(&cancel)?;
    let worker_cancel = cancel.clone();
    let task = tokio::task::spawn_blocking(move || work(worker_cancel));
    tokio::select! {
        result = task => result.map_err(|error| error.to_string())?,
        _ = cancel.cancelled() => Err("CANCELLED".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    /// 建立只含 Lease 的 fixture，证明 Source helper 不需要任何 Registry 或 UI 状态。
    fn lease(root: &Path) -> WorkspaceLease {
        WorkspaceLease {
            workspace_id: "workspace-a".into(),
            canonical_root: root.canonicalize().unwrap(),
            generation: 12,
        }
    }

    /// Source 组合 API 必须直接复用 Git 已使用的唯一 WorkspacePathResolver。
    #[test]
    fn source_path_helpers_delegate_to_the_shared_workspace_path_authority() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("src/nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("file.rs"), "fn main() {}\n").unwrap();
        let lease = lease(directory.path());

        assert_eq!(
            resolve_relative_path(&lease, "src/nested/./file.rs").unwrap(),
            WorkspacePathResolver::new(&lease)
                .resolve("src/nested/./file.rs")
                .unwrap()
        );
        assert_eq!(
            workspace_root(&lease).unwrap(),
            WorkspacePathResolver::new(&lease).root().unwrap()
        );
    }

    /// 已取消的 Token 必须在 spawn blocking work 前返回，闭包不得开始。
    #[tokio::test]
    async fn cancellation_before_blocking_work_prevents_start() {
        let cancel = CancellationToken::new();
        let started = Arc::new(AtomicBool::new(false));
        cancel.cancel();
        let observed = Arc::clone(&started);

        assert_eq!(
            run_blocking_cancellable(cancel, move |_| {
                observed.store(true, Ordering::SeqCst);
                Ok(())
            })
            .await,
            Err("CANCELLED".into())
        );
        assert!(!started.load(Ordering::SeqCst));
    }

    /// blocking 遍历循环复用同一 Token，并在取消后自行停止。
    #[tokio::test]
    async fn cancellation_stops_a_blocking_traversal_loop() {
        let cancel = CancellationToken::new();
        let entered = Arc::new(AtomicBool::new(false));
        let observed = Arc::new(AtomicBool::new(false));
        let worker_entered = Arc::clone(&entered);
        let worker_observed = Arc::clone(&observed);
        let task = tokio::spawn(run_blocking_cancellable(
            cancel.clone(),
            move |worker_cancel| -> Result<(), String> {
                worker_entered.store(true, Ordering::SeqCst);
                loop {
                    if check_cancelled(&worker_cancel).is_err() {
                        worker_observed.store(true, Ordering::SeqCst);
                        return Err("CANCELLED".into());
                    }
                    std::thread::yield_now();
                }
            },
        ));
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        cancel.cancel();

        assert_eq!(task.await.unwrap(), Err("CANCELLED".into()));
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !observed.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    /// UTF-8 hard budget 以字节计算，截断点必须保持有效字符边界。
    #[test]
    fn bounded_text_honors_hard_budget_without_splitting_utf8() {
        assert_eq!(
            bounded_text("A中B", 3),
            BoundedText {
                text: "A".into(),
                truncated: true,
            }
        );
        assert_eq!(
            bounded_text("A中B", 4),
            BoundedText {
                text: "A中".into(),
                truncated: true,
            }
        );
        assert_eq!(
            bounded_text("A中B", 5),
            BoundedText {
                text: "A中B".into(),
                truncated: false,
            }
        );
    }

    /// provenance 的序列化只保留冻结的 id 与 generation。
    #[test]
    fn provenance_contains_only_id_and_generation() {
        let directory = tempfile::tempdir().unwrap();
        let value = serde_json::to_value(workspace_provenance(&lease(directory.path()))).unwrap();

        assert_eq!(
            value,
            serde_json::json!({"workspace":{"id":"workspace-a","generation":12}})
        );
        assert!(
            !value
                .to_string()
                .contains(&directory.path().to_string_lossy().to_string())
        );
    }

    /// 已捕获 Lease 后的外部 selection 变化不会成为 Source helper 输入或影响解析结果。
    #[test]
    fn captured_lease_helpers_do_not_depend_on_later_registry_or_ui_state() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("src")).unwrap();
        let captured = lease(directory.path());
        let mut simulated_desktop_selection = "workspace-b";
        assert_eq!(simulated_desktop_selection, "workspace-b");
        simulated_desktop_selection = "workspace-c";

        let path = resolve_relative_path(&captured, "src/future.rs").unwrap();

        assert_eq!(simulated_desktop_selection, "workspace-c");
        assert_eq!(
            path,
            workspace_root(&captured).unwrap().join("src/future.rs")
        );
        assert_eq!(workspace_provenance(&captured).workspace.id, "workspace-a");
    }
}

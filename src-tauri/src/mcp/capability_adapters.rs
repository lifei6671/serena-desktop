//! 无状态 Source/Git Adapter：只接收 Manager 已验证的 Lease，绝不建立 RuntimeSlot 或读取全局 Workspace。

use super::{git, source_find, source_list, source_read, source_search};
use crate::{
    workspace_capability::{
        CapabilityActionDescriptor, CapabilityFuture, CapabilityInstallation,
        CapabilityInstallationState, CapabilityObservation, CapabilityPreparationPolicy,
        CapabilityPrepareAction, CapabilityPrepareResult, CapabilityProviderError,
        CapabilityProviderErrorCode, CapabilityReadinessProbe, CapabilityReadinessState,
        CapabilityRuntimeHandle, CapabilityRuntimeModel, CapabilityRuntimePolicy,
        CapabilityRuntimeState, CapabilityStageDescriptor, CapabilityStageRequirement,
        CapabilityStageState, CapabilityStopFailure, StopEvidence, WorkspaceCapabilityDescriptor,
        WorkspaceCapabilityProvider, WorkspaceCapabilityProviderId, WorkspaceToolCall,
        WorkspaceToolResult,
    },
    workspace_resolver::WorkspaceLease,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// 将已冻结的工具业务成功或错误投影封装在 Provider result 内，避免把业务错误误映射为 Runtime 失败。
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdapterToolResult {
    result: Option<Value>,
    error: Option<String>,
}

impl AdapterToolResult {
    /// 生成保持原 MCP 返回值的成功 envelope。
    fn success(result: Value) -> WorkspaceToolResult {
        WorkspaceToolResult {
            result: serde_json::to_value(Self {
                result: Some(result),
                error: None,
            })
            .expect("adapter result must serialize"),
        }
    }

    /// 生成保持既有稳定字符串的业务错误 envelope。
    fn error(error: String) -> WorkspaceToolResult {
        WorkspaceToolResult {
            result: serde_json::to_value(Self {
                result: None,
                error: Some(error),
            })
            .expect("adapter error must serialize"),
        }
    }
}

/// 将无状态 Adapter 的内部 envelope 还原为既有 MCP 成功或错误行为。
pub(crate) fn decode_tool_result(result: WorkspaceToolResult) -> Result<Value, String> {
    let envelope: AdapterToolResult = serde_json::from_value(result.result)
        .map_err(|_| "WORKSPACE_CAPABILITY_CONTRACT_ERROR".to_owned())?;
    match (envelope.result, envelope.error) {
        (Some(result), None) => Ok(result),
        (None, Some(error)) => Err(error),
        _ => Err("WORKSPACE_CAPABILITY_CONTRACT_ERROR".into()),
    }
}

/// 构造不需要 Runtime、准备或持久化 readiness 的通用 descriptor。
fn stateless_descriptor(
    provider_id: &str,
    display_name: &str,
    runtime_model: CapabilityRuntimeModel,
    tool_names: impl IntoIterator<Item = &'static str>,
) -> WorkspaceCapabilityDescriptor {
    WorkspaceCapabilityDescriptor {
        provider_id: WorkspaceCapabilityProviderId::new(provider_id),
        display_name: display_name.into(),
        tool_names: tool_names.into_iter().map(str::to_owned).collect(),
        runtime_model,
        readiness_probe: CapabilityReadinessProbe::None,
        preparation_policy: CapabilityPreparationPolicy::None,
        stage_descriptors: vec![CapabilityStageDescriptor {
            id: "ready".into(),
            display_name: "Ready".into(),
            requirement: CapabilityStageRequirement::Optional,
        }],
        action_descriptors: Vec::<CapabilityActionDescriptor>::new(),
        runtime_policy: CapabilityRuntimePolicy {
            max_instances: 0,
            idle_timeout_ms: 0,
            per_slot_concurrency: 1,
        },
    }
}

/// 无状态 Adapter 共用的无 Runtime 生命周期实现。
macro_rules! stateless_lifecycle {
    () => {
        fn probe_installation(
            &self,
        ) -> CapabilityFuture<'_, Result<CapabilityInstallation, CapabilityProviderError>> {
            Box::pin(async {
                Ok(CapabilityInstallation {
                    state: CapabilityInstallationState::Installed,
                    detected_version: None,
                })
            })
        }

        fn observe_readiness(
            &self,
            _lease: WorkspaceLease,
        ) -> CapabilityFuture<'_, Result<CapabilityObservation, CapabilityProviderError>> {
            let provider_id = self.descriptor.provider_id.clone();
            Box::pin(async move {
                Ok(CapabilityObservation {
                    provider_id,
                    installation: CapabilityInstallationState::Installed,
                    readiness: CapabilityReadinessState::Ready,
                    runtime_state: CapabilityRuntimeState::Stopped,
                    checked_at: 0,
                    stages: vec![crate::workspace_capability::CapabilityStage {
                        id: "ready".into(),
                        display_name: "Ready".into(),
                        state: CapabilityStageState::Ready,
                        requirement: CapabilityStageRequirement::Optional,
                        message_code: None,
                    }],
                    actions: vec![],
                })
            })
        }

        fn prepare<'a>(
            &'a self,
            _lease: WorkspaceLease,
            _action: CapabilityPrepareAction,
            _activity: &'a dyn crate::workspace_capability::CapabilityActivitySink,
        ) -> CapabilityFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>> {
            Box::pin(async {
                Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::ContractError,
                })
            })
        }

        fn start(
            &self,
            _lease: WorkspaceLease,
        ) -> CapabilityFuture<'_, Result<CapabilityRuntimeHandle, CapabilityProviderError>> {
            Box::pin(async {
                Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::OperationFailed,
                })
            })
        }

        fn stop(
            &self,
            runtime: CapabilityRuntimeHandle,
        ) -> CapabilityFuture<'_, Result<StopEvidence, CapabilityStopFailure>> {
            Box::pin(async move {
                Err(CapabilityStopFailure {
                    runtime,
                    error: CapabilityProviderError {
                        code: CapabilityProviderErrorCode::ContractError,
                    },
                })
            })
        }
    };
}

/// Source 四个已公开 Rust read tool 的 in-process Adapter。
pub(crate) struct SourceCapabilityProvider {
    descriptor: WorkspaceCapabilityDescriptor,
}

impl SourceCapabilityProvider {
    /// 构造仅声明四个公开 read tool 的 Provider；P2C local write 不加入 Remote surface。
    pub(crate) fn new() -> Self {
        Self {
            descriptor: stateless_descriptor(
                "source",
                "Source",
                CapabilityRuntimeModel::InProcess,
                [
                    "source_read_file",
                    "source_list_dir",
                    "source_find_file",
                    "source_search_pattern",
                ],
            ),
        }
    }
}

impl WorkspaceCapabilityProvider for SourceCapabilityProvider {
    fn descriptor(&self) -> &WorkspaceCapabilityDescriptor {
        &self.descriptor
    }

    stateless_lifecycle!();

    fn call<'a>(
        &'a self,
        lease: &'a WorkspaceLease,
        runtime: Option<&'a CapabilityRuntimeHandle>,
        tool: WorkspaceToolCall,
    ) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>> {
        Box::pin(async move {
            if runtime.is_some() {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::RuntimeIdentityMismatch,
                });
            }
            let arguments =
                serde_json::from_value(tool.arguments).map_err(|_| CapabilityProviderError {
                    code: CapabilityProviderErrorCode::ContractError,
                })?;
            let result = match tool.tool_name.as_str() {
                "source_read_file" => source_read::read(lease, arguments, tool.cancellation).await,
                "source_list_dir" => source_list::list(lease, arguments, tool.cancellation).await,
                "source_find_file" => source_find::find(lease, arguments, tool.cancellation).await,
                "source_search_pattern" => {
                    source_search::search(lease, arguments, tool.cancellation).await
                }
                _ => {
                    return Err(CapabilityProviderError {
                        code: CapabilityProviderErrorCode::ContractError,
                    });
                }
            };
            Ok(match result {
                Ok(result) => AdapterToolResult::success(result),
                Err(error) => AdapterToolResult::error(error),
            })
        })
    }
}

/// Git 六个既有 Lease-rooted tool 的 in-process Adapter。
pub(crate) struct GitCapabilityProvider {
    descriptor: WorkspaceCapabilityDescriptor,
}

impl GitCapabilityProvider {
    /// 构造不保存 Root 的 Git Provider；每次 call 都只使用参数中的服务端 Lease。
    pub(crate) fn new() -> Self {
        Self {
            descriptor: stateless_descriptor(
                "git",
                "Git",
                CapabilityRuntimeModel::StatelessCommand,
                super::registry::GITS.iter().copied(),
            ),
        }
    }
}

impl WorkspaceCapabilityProvider for GitCapabilityProvider {
    fn descriptor(&self) -> &WorkspaceCapabilityDescriptor {
        &self.descriptor
    }

    stateless_lifecycle!();

    fn call<'a>(
        &'a self,
        lease: &'a WorkspaceLease,
        runtime: Option<&'a CapabilityRuntimeHandle>,
        tool: WorkspaceToolCall,
    ) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>> {
        Box::pin(async move {
            if runtime.is_some() {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::RuntimeIdentityMismatch,
                });
            }
            let args: git::GitArgs =
                serde_json::from_value(tool.arguments).map_err(|_| CapabilityProviderError {
                    code: CapabilityProviderErrorCode::ContractError,
                })?;
            // 防御 Adapter 被绕过时将 A 的 payload 配对 B 的 Lease；Broker 正常路径已在 Resolver 前保证此条件。
            if args.workspace_id != lease.workspace_id {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::ContractError,
                });
            }
            let output = git::call(&tool.tool_name, lease, args, tool.cancellation).await;
            Ok(match output {
                Ok(output) => {
                    let mut result = json!({
                        "workspace":{"id":lease.workspace_id,"generation":lease.generation},
                        "text":output.text,
                        "truncated":output.truncated,
                    });
                    if output.truncated {
                        result["hint"] = json!("请缩小路径、行范围或日志数量");
                    }
                    AdapterToolResult::success(result)
                }
                Err(error) => AdapterToolResult::error(error),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, process::Command};
    use tokio_util::sync::CancellationToken;

    /// 构造只由服务端建立的 Lease fixture，不把 Root 放入 Tool payload。
    fn lease(root: &Path, workspace_id: &str) -> WorkspaceLease {
        WorkspaceLease {
            workspace_id: workspace_id.into(),
            canonical_root: root.canonicalize().unwrap(),
            generation: 1,
        }
    }

    #[tokio::test]
    /// 同名路径在不同根目录中必须由传入 Lease 区分，Source Adapter 不可回退全局选择。
    async fn source_adapter_reads_only_the_explicit_lease_root() {
        let directory = tempfile::tempdir().unwrap();
        let root_a = directory.path().join("workspace-a");
        let root_b = directory.path().join("workspace-b");
        fs::create_dir_all(root_a.join("src")).unwrap();
        fs::create_dir_all(root_b.join("src")).unwrap();
        fs::write(root_a.join("src/marker.rs"), "from-a\n").unwrap();
        fs::write(root_b.join("src/marker.rs"), "from-b\n").unwrap();
        let provider = SourceCapabilityProvider::new();

        let result = provider
            .call(
                &lease(&root_a, "workspace-a"),
                None,
                WorkspaceToolCall {
                    tool_name: "source_read_file".into(),
                    arguments: json!({"relative_path":"src/marker.rs"}),
                    cancellation: CancellationToken::new(),
                },
            )
            .await
            .unwrap();
        let result = decode_tool_result(result).unwrap();

        assert_eq!(result["workspace"]["id"], "workspace-a");
        assert!(result["text"].as_str().unwrap().contains("from-a"));
        assert!(!result["text"].as_str().unwrap().contains("from-b"));
    }

    #[tokio::test]
    /// Git payload 与 Lease workspace identity 不一致时 fail closed，不能将 A 的参数在 B 根目录执行。
    async fn git_adapter_rejects_workspace_id_mismatch_before_command_execution() {
        let directory = tempfile::tempdir().unwrap();
        let provider = GitCapabilityProvider::new();
        let error = provider
            .call(
                &lease(directory.path(), "workspace-a"),
                None,
                WorkspaceToolCall {
                    tool_name: "git_status".into(),
                    arguments: json!({"workspaceId":"workspace-b"}),
                    cancellation: CancellationToken::new(),
                },
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, CapabilityProviderErrorCode::ContractError);
    }

    #[test]
    /// Source/Git Provider 分别冻结为 in-process 与 stateless command，P2C write 不能因 Adapter 注册进入 Remote surface。
    fn source_descriptor_keeps_write_tools_out_of_the_adapter_surface() {
        let source = SourceCapabilityProvider::new().descriptor().clone();
        let git = GitCapabilityProvider::new().descriptor().clone();
        assert_eq!(source.runtime_model, CapabilityRuntimeModel::InProcess);
        assert_eq!(git.runtime_model, CapabilityRuntimeModel::StatelessCommand);
        assert_eq!(source.tool_names.len(), 4);
        for name in crate::mcp::source_write_domain::SourceWriteTool::ALL {
            assert!(!source.tool_names.contains(&name.code().to_owned()));
        }
    }

    #[tokio::test]
    /// Git Adapter 必须把公开 `path` 原样交给既有 WorkspacePathResolver，而非复制或改名为 relative_path。
    async fn git_adapter_preserves_workspace_relative_path_contract() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repository");
        assert!(
            Command::new("git")
                .args(["init"])
                .arg(&repository)
                .status()
                .unwrap()
                .success()
        );
        fs::write(repository.join("inside.txt"), "before\n").unwrap();
        for args in [
            vec!["add", "inside.txt"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "adapter fixture",
                "--no-gpg-sign",
            ],
        ] {
            assert!(
                Command::new("git")
                    .current_dir(&repository)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(repository.join("inside.txt"), "after\n").unwrap();
        let lease = lease(&repository, "workspace-a");
        let provider = GitCapabilityProvider::new();
        for (tool_name, arguments, expected) in [
            (
                "git_diff",
                json!({"workspaceId":"workspace-a"}),
                "inside.txt",
            ),
            (
                "git_diff",
                json!({"workspaceId":"workspace-a", "path":"inside.txt"}),
                "inside.txt",
            ),
            (
                "git_log",
                json!({"workspaceId":"workspace-a", "path":"inside.txt"}),
                "adapter fixture",
            ),
        ] {
            let result = provider
                .call(
                    &lease,
                    None,
                    WorkspaceToolCall {
                        tool_name: tool_name.into(),
                        arguments,
                        cancellation: CancellationToken::new(),
                    },
                )
                .await
                .unwrap();
            assert!(
                decode_tool_result(result).unwrap()["text"]
                    .as_str()
                    .unwrap()
                    .contains(expected)
            );
        }
        for path in [
            "/outside",
            "C:/outside",
            "C:\\outside",
            "\\\\server\\share",
            "..",
            "../outside",
            "..\\outside",
            ".",
        ] {
            let result = provider
                .call(
                    &lease,
                    None,
                    WorkspaceToolCall {
                        tool_name: "git_diff".into(),
                        arguments: json!({"workspaceId":"workspace-a", "path":path}),
                        cancellation: CancellationToken::new(),
                    },
                )
                .await
                .unwrap();
            assert!(
                decode_tool_result(result)
                    .unwrap_err()
                    .starts_with("INVALID_PATH"),
                "{path}"
            );
        }
    }
}

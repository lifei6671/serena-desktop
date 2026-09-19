//! CodeGraph readiness、显式索引与 Workspace-scoped query Runtime Adapter；不承载全局 Workspace 或 Remote Tool。

use crate::{
    mcp::process,
    workspace_capability::{
        CapabilityAction, CapabilityActionAuthority, CapabilityActionDescriptor,
        CapabilityActionExecution, CapabilityActivitySink, CapabilityFuture,
        CapabilityInstallation, CapabilityInstallationState, CapabilityObservation,
        CapabilityPreparationPolicy, CapabilityPrepareAction, CapabilityPrepareResult,
        CapabilityProviderError, CapabilityProviderErrorCode, CapabilityReadinessProbe,
        CapabilityReadinessState, CapabilityRuntimeHandle, CapabilityRuntimeModel,
        CapabilityRuntimePolicy, CapabilityRuntimeState, CapabilityStage,
        CapabilityStageDescriptor, CapabilityStageRequirement, CapabilityStageState,
        CapabilityStopFailure, StopEvidence, WorkspaceCapabilityDescriptor,
        WorkspaceCapabilityProvider, WorkspaceCapabilityProviderId, WorkspaceToolCall,
        WorkspaceToolResult,
    },
    workspace_resolver::WorkspaceLease,
};
use rmcp::{
    RoleClient, ServiceExt,
    model::CallToolRequestParams,
    service::RunningService,
    transport::{TokioChildProcess, which_command},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    ffi::OsString,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{io::AsyncReadExt, sync::Mutex};
use tokio_util::sync::CancellationToken;

/// 单次 CodeGraph CLI 命令的受控输入；唯一工作目录和路径均来自已解析 Lease。
#[derive(Clone, Debug, PartialEq, Eq)]
struct CodeGraphCommand {
    args: Vec<OsString>,
    current_dir: PathBuf,
}

/// 运行器只存在于 Adapter 内部，以便测试精确冻结 argv 且不调用真实业务 Workspace。
type CodeGraphCommandRunner = Arc<
    dyn Fn(CodeGraphCommand) -> CapabilityFuture<'static, Result<String, CapabilityProviderError>>
        + Send
        + Sync,
>;

/// 安装探测与命令执行分开，避免 health 观察把 binary 缺失伪装成 Workspace readiness。
type InstallationProbe = Arc<dyn Fn() -> CapabilityInstallation + Send + Sync>;

/// Runtime map 的唯一键只来自已解析 Lease，绝不接受 Tool payload 或 Desktop selection。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CodeGraphRuntimeKey {
    workspace_id: String,
    generation: u64,
}

impl CodeGraphRuntimeKey {
    /// 将 Lease identity 固定为 Provider-private runtime lookup key。
    fn from_lease(lease: &WorkspaceLease) -> Self {
        Self {
            workspace_id: lease.workspace_id.clone(),
            generation: lease.generation,
        }
    }
}

/// Provider 内部 MCP client port；Manager 只能看到 opaque handle。
trait CodeGraphRuntimeClient: Send + Sync {
    /// 调用唯一允许的 upstream tool，并收敛 transport 原始错误。
    fn explore<'a>(
        &'a self,
        arguments: Value,
    ) -> CapabilityFuture<'a, Result<String, CapabilityProviderError>>;
}

/// 生产 MCP client 与 stderr drain 同生共死，drop 即关闭 transport 和受管 child。
struct CodeGraphClient {
    service: RunningService<RoleClient, ()>,
    stderr_task: tokio::task::JoinHandle<()>,
}

impl Drop for CodeGraphClient {
    fn drop(&mut self) {
        self.service.cancellation_token().cancel();
        self.stderr_task.abort();
    }
}

impl CodeGraphRuntimeClient for CodeGraphClient {
    /// 将 MCP 响应严格投影为既有纯文本结果，不泄露上游 transport 细节。
    fn explore<'a>(
        &'a self,
        arguments: Value,
    ) -> CapabilityFuture<'a, Result<String, CapabilityProviderError>> {
        Box::pin(async move {
            let response = self
                .service
                .call_tool(
                    CallToolRequestParams::new("codegraph_explore")
                        .with_arguments(arguments.as_object().cloned().unwrap_or_default()),
                )
                .await
                .map_err(|_| deferred_operation())?;
            if response.is_error == Some(true) {
                return Err(deferred_operation());
            }
            let value = serde_json::to_value(response).map_err(|_| deferred_operation())?;
            let texts = value["content"]
                .as_array()
                .ok_or_else(deferred_operation)?
                .iter()
                .map(|item| {
                    (item["type"] == "text")
                        .then(|| item["text"].as_str())
                        .flatten()
                        .map(str::to_owned)
                        .ok_or_else(deferred_operation)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let text = texts.join("\n");
            if text.starts_with("No CodeGraph project is loaded")
                || text.starts_with("The project at ") && text.contains("isn't indexed")
            {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::NotPrepared,
                });
            }
            (text.len() <= 262_144)
                .then_some(text)
                .ok_or_else(deferred_operation)
        })
    }
}

/// 运行时工厂可替换，以在单元测试中不启动真实 CodeGraph。
type RuntimeStarter = Arc<
    dyn Fn(
            WorkspaceLease,
        ) -> CapabilityFuture<
            'static,
            Result<Arc<dyn CodeGraphRuntimeClient>, CapabilityProviderError>,
        > + Send
        + Sync,
>;

/// 真实 `status --json` 的最小稳定字段；额外 CLI 字段刻意忽略以兼容同一已验证 schema 的扩展。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodeGraphStatus {
    initialized: bool,
    project_path: PathBuf,
    pending_changes: Option<CodeGraphPendingChanges>,
    index: Option<CodeGraphIndex>,
}

/// CLI 用三个计数描述未同步内容；任一非零即为 stale evidence。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodeGraphPendingChanges {
    added: u64,
    modified: u64,
    removed: u64,
}

/// `reindexRecommended` 与 `state` 都嵌在实机 1.6.0 的 index 对象内。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodeGraphIndex {
    reindex_recommended: bool,
    state: String,
}

/// 只保留 Provider 对 Manager 可见的安全 readiness 投影。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusProjection {
    NotPrepared,
    Ready,
    Degraded,
}

impl StatusProjection {
    /// 转换到公共 readiness 枚举，不把 JSON、Root 或命令错误越过边界。
    fn readiness(self) -> CapabilityReadinessState {
        match self {
            Self::NotPrepared => CapabilityReadinessState::NotPrepared,
            Self::Ready => CapabilityReadinessState::Ready,
            Self::Degraded => CapabilityReadinessState::Degraded,
        }
    }

    /// 生成唯一 index stage 的安全状态与固定消息代码。
    fn stage(self) -> (CapabilityStageState, Option<String>) {
        match self {
            Self::NotPrepared => (
                CapabilityStageState::Absent,
                Some("CAPABILITY_STAGE_NOT_PREPARED".into()),
            ),
            Self::Ready => (CapabilityStageState::Ready, None),
            Self::Degraded => (
                CapabilityStageState::Stale,
                Some("CAPABILITY_STAGE_STALE".into()),
            ),
        }
    }
}

/// CodeGraph 的 Phase 2D shell；descriptor 不发布任何 query tool。
pub(crate) struct CodeGraphCapabilityProvider {
    descriptor: WorkspaceCapabilityDescriptor,
    installation_probe: InstallationProbe,
    command_runner: CodeGraphCommandRunner,
    runtime_starter: RuntimeStarter,
    runtimes: Mutex<HashMap<CodeGraphRuntimeKey, Arc<dyn CodeGraphRuntimeClient>>>,
}

impl CodeGraphCapabilityProvider {
    /// 构造生产 Provider；构造不探测 binary、不运行 CLI，也不读写任何 Workspace。
    pub(crate) fn new() -> Self {
        Self::with_runtime(
            Arc::new(discover_installation),
            Arc::new(run_command),
            Arc::new(start_runtime),
        )
    }

    /// 用可控 probe 构造 Provider，供本模块测试验证 JSON、argv 和取消边界。
    fn with_probes(
        installation_probe: InstallationProbe,
        command_runner: CodeGraphCommandRunner,
    ) -> Self {
        Self::with_runtime(installation_probe, command_runner, Arc::new(start_runtime))
    }

    /// 用可控 status 与 Runtime 工厂构造 Provider，保持生产启动边界可独立测试。
    fn with_runtime(
        installation_probe: InstallationProbe,
        command_runner: CodeGraphCommandRunner,
        runtime_starter: RuntimeStarter,
    ) -> Self {
        Self {
            descriptor: WorkspaceCapabilityDescriptor {
                provider_id: WorkspaceCapabilityProviderId::new("codegraph"),
                display_name: "CodeGraph".into(),
                tool_names: vec!["codegraph_explore".into()],
                runtime_model: CapabilityRuntimeModel::WorkspaceScopedProcess,
                readiness_probe: CapabilityReadinessProbe::Required,
                preparation_policy: CapabilityPreparationPolicy::ExplicitOnly,
                stage_descriptors: vec![CapabilityStageDescriptor {
                    id: "index".into(),
                    display_name: "代码图索引".into(),
                    requirement: CapabilityStageRequirement::Required,
                }],
                action_descriptors: vec![
                    action_descriptor("build_index", "建立索引"),
                    action_descriptor("update_index", "更新索引"),
                    action_descriptor("rebuild_index", "重建索引"),
                ],
                // P0-007 的实机双进程、RSS 与 stop/crash 证据冻结的首版 Runtime 策略。
                runtime_policy: CapabilityRuntimePolicy {
                    max_instances: 2,
                    idle_timeout_ms: 300_000,
                    per_slot_concurrency: 1,
                },
            },
            installation_probe,
            command_runner,
            runtime_starter,
            runtimes: Mutex::new(HashMap::new()),
        }
    }

    /// 由 Descriptor 投影 action，Provider 不得篡改 Local Human 或 execution 契约。
    fn actions(&self) -> Vec<CapabilityAction> {
        self.descriptor
            .action_descriptors
            .iter()
            .map(|action| CapabilityAction {
                id: action.action_id.clone(),
                display_name: action.display_name.clone(),
                authority: action.authority,
                execution: action.execution,
            })
            .collect()
    }

    /// 从唯一合法 authority 运行 status；不读取 .codegraph、Desktop selection 或 caller root。
    async fn status(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<StatusProjection, CapabilityProviderError> {
        let output = (self.command_runner)(status_command(lease)).await?;
        project_status(&output, lease)
    }

    /// 启动前复用同一 status authority，禁止 Runtime acquire 借机 init/sync/index。
    async fn require_ready(&self, lease: &WorkspaceLease) -> Result<(), CapabilityProviderError> {
        match self.status(lease).await? {
            StatusProjection::Ready => Ok(()),
            StatusProjection::NotPrepared | StatusProjection::Degraded => {
                Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::NotPrepared,
                })
            }
        }
    }

    /// 从 status projection 生成安全 observation；只暴露统一 stage 和 descriptor actions。
    fn observation(&self, status: StatusProjection) -> CapabilityObservation {
        let (state, message_code) = status.stage();
        CapabilityObservation {
            provider_id: self.descriptor.provider_id.clone(),
            installation: CapabilityInstallationState::Installed,
            readiness: status.readiness(),
            runtime_state: CapabilityRuntimeState::Stopped,
            checked_at: checked_at(),
            stages: vec![CapabilityStage {
                id: "index".into(),
                display_name: "代码图索引".into(),
                state,
                requirement: CapabilityStageRequirement::Required,
                message_code,
            }],
            actions: self.actions(),
        }
    }
}

/// 三个显式动作共享冻结的 Local Human / provider_prepare / cold-runtime 语义。
fn action_descriptor(action_id: &str, display_name: &str) -> CapabilityActionDescriptor {
    CapabilityActionDescriptor {
        action_id: action_id.into(),
        display_name: display_name.into(),
        authority: CapabilityActionAuthority::LocalHuman,
        execution: CapabilityActionExecution::ProviderPrepare,
        warm_runtime: false,
    }
}

/// 只用 PATH discovery 判断 binary 是否可执行；不启动 CLI 或推断 index 目录。
fn discover_installation() -> CapabilityInstallation {
    match which_command("codegraph") {
        Ok(_) => CapabilityInstallation {
            state: CapabilityInstallationState::Installed,
            detected_version: None,
        },
        Err(_) => CapabilityInstallation {
            state: CapabilityInstallationState::NotInstalled,
            detected_version: None,
        },
    }
}

/// 生产 runner 使用受控 process helper，限制输出与时长，并在 future drop 时杀死子进程。
fn run_command(
    command: CodeGraphCommand,
) -> CapabilityFuture<'static, Result<String, CapabilityProviderError>> {
    Box::pin(async move {
        let mut child = which_command("codegraph").map_err(|_| CapabilityProviderError {
            code: CapabilityProviderErrorCode::Unavailable,
        })?;
        // which_command 只定位可执行文件，不承诺 stdio 已配置；受控 runner 需要 pipe 才能安全读取 status。
        child
            .args(&command.args)
            .current_dir(command.current_dir)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        child.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        process::run(
            child,
            64 * 1024,
            Duration::from_secs(30),
            CancellationToken::new(),
        )
        .await
        .map(|output| output.text)
        .map_err(|_| CapabilityProviderError {
            code: CapabilityProviderErrorCode::OperationFailed,
        })
    })
}

/// 以官方 CLI 的 direct MCP mode 启动受 Slot 管理的 child；默认 detached daemon 不满足 stop ownership。
fn start_runtime(
    lease: WorkspaceLease,
) -> CapabilityFuture<'static, Result<Arc<dyn CodeGraphRuntimeClient>, CapabilityProviderError>> {
    Box::pin(async move {
        let mut command = which_command("codegraph").map_err(|_| CapabilityProviderError {
            code: CapabilityProviderErrorCode::Unavailable,
        })?;
        command
            .args(["serve", "--mcp", "--path"])
            .arg(&lease.canonical_root)
            .current_dir(&lease.canonical_root)
            // 1.6.0 默认 daemon 会逃逸 RuntimeSlot stop；direct mode 的 child 才可被 handle 独占。
            .env("CODEGRAPH_NO_DAEMON", "1")
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        let (transport, stderr) = TokioChildProcess::builder(command)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| deferred_operation())?;
        let stderr_task = tokio::spawn(async move {
            if let Some(mut stderr) = stderr {
                let mut remaining = 8 * 1024;
                let mut bytes = [0_u8; 1024];
                while let Ok(read) = stderr.read(&mut bytes).await {
                    if read == 0 || remaining == 0 {
                        break;
                    }
                    remaining -= read.min(remaining);
                }
            }
        });
        let service = ().serve(transport).await.map_err(|_| deferred_operation())?;
        let tools = service
            .list_all_tools()
            .await
            .map_err(|_| deferred_operation())?;
        if !tools.iter().any(|tool| tool.name == "codegraph_explore") {
            return Err(deferred_operation());
        }
        Ok(Arc::new(CodeGraphClient {
            service,
            stderr_task,
        }) as Arc<dyn CodeGraphRuntimeClient>)
    })
}

/// 生成状态读取的精确 argv；path 既是唯一 path argument，也是受控 current_dir。
fn status_command(lease: &WorkspaceLease) -> CodeGraphCommand {
    command_for(lease, [OsString::from("status"), OsString::from("--json")])
}

/// 生成显式索引动作的精确 argv；所有动作都以 canonical root 为唯一 path argument。
fn prepare_command(lease: &WorkspaceLease, action_id: &str) -> Option<CodeGraphCommand> {
    let args = match action_id {
        "build_index" => vec![OsString::from("init"), OsString::from("--yes")],
        "update_index" => vec![OsString::from("sync")],
        "rebuild_index" => vec![OsString::from("index")],
        _ => return None,
    };
    Some(command_for(lease, args))
}

/// 将已冻结 Lease root 追加为命令的唯一 path argument，禁止 caller root 进入此层。
fn command_for(
    lease: &WorkspaceLease,
    args: impl IntoIterator<Item = OsString>,
) -> CodeGraphCommand {
    let mut args: Vec<_> = args.into_iter().collect();
    args.push(lease.canonical_root.clone().into_os_string());
    CodeGraphCommand {
        args,
        current_dir: lease.canonical_root.clone(),
    }
}

/// 解析实机 JSON 并验证 Root identity；任意缺字段、JSON 错误或未知 index state 都 fail closed。
fn project_status(
    text: &str,
    lease: &WorkspaceLease,
) -> Result<StatusProjection, CapabilityProviderError> {
    let status: CodeGraphStatus =
        serde_json::from_str(text).map_err(|_| CapabilityProviderError {
            code: CapabilityProviderErrorCode::OperationFailed,
        })?;
    let project_root = status
        .project_path
        .canonicalize()
        .map_err(|_| CapabilityProviderError {
            code: CapabilityProviderErrorCode::OperationFailed,
        })?;
    if project_root != lease.canonical_root {
        return Err(CapabilityProviderError {
            code: CapabilityProviderErrorCode::ContractError,
        });
    }
    if !status.initialized {
        return Ok(StatusProjection::NotPrepared);
    }
    let pending = status.pending_changes.ok_or(CapabilityProviderError {
        code: CapabilityProviderErrorCode::OperationFailed,
    })?;
    let index = status.index.ok_or(CapabilityProviderError {
        code: CapabilityProviderErrorCode::OperationFailed,
    })?;
    if index.state != "complete" {
        return Err(CapabilityProviderError {
            code: CapabilityProviderErrorCode::OperationFailed,
        });
    }
    if pending.added != 0
        || pending.modified != 0
        || pending.removed != 0
        || index.reindex_recommended
    {
        Ok(StatusProjection::Degraded)
    } else {
        Ok(StatusProjection::Ready)
    }
}

/// 返回仅用于 health freshness 的 UTC 毫秒，不让系统时钟异常破坏 fail-closed 投影。
fn checked_at() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

impl WorkspaceCapabilityProvider for CodeGraphCapabilityProvider {
    /// 返回 immutable descriptor；不会执行 discovery、status 或任何 index action。
    fn descriptor(&self) -> &WorkspaceCapabilityDescriptor {
        &self.descriptor
    }

    /// 仅探测 binary；磁盘上的 `.codegraph` 不能代替已安装状态。
    fn probe_installation(
        &self,
    ) -> CapabilityFuture<'_, Result<CapabilityInstallation, CapabilityProviderError>> {
        let installation = (self.installation_probe)();
        Box::pin(async move { Ok(installation) })
    }

    /// 仅用 `status --json` 投影 readiness；命令失败与 schema 错误全部 fail closed。
    fn observe_readiness(
        &self,
        lease: WorkspaceLease,
    ) -> CapabilityFuture<'_, Result<CapabilityObservation, CapabilityProviderError>> {
        Box::pin(async move {
            if (self.installation_probe)().state != CapabilityInstallationState::Installed {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::Unavailable,
                });
            }
            let status = self.status(&lease).await?;
            Ok(self.observation(status))
        })
    }

    /// 仅执行 Descriptor 声明的 Local Human 动作，并强制用 post-action status 验证结果。
    fn prepare<'a>(
        &'a self,
        lease: WorkspaceLease,
        action: CapabilityPrepareAction,
        _activity: &'a dyn CapabilityActivitySink,
    ) -> CapabilityFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>> {
        Box::pin(async move {
            if (self.installation_probe)().state != CapabilityInstallationState::Installed {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::Unavailable,
                });
            }
            let command =
                prepare_command(&lease, &action.action_id).ok_or(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::ContractError,
                })?;
            (self.command_runner)(command).await?;
            let status = self.status(&lease).await?;
            let valid = match action.action_id.as_str() {
                "build_index" => status != StatusProjection::NotPrepared,
                "update_index" => status != StatusProjection::NotPrepared,
                "rebuild_index" => status == StatusProjection::Ready,
                _ => false,
            };
            if !valid {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::OperationFailed,
                });
            }
            Ok(CapabilityPrepareResult {
                readiness: status.readiness(),
            })
        })
    }

    /// 先由 status 证明 root/readiness，再启动固定绑定 canonical root 的独立 direct MCP server。
    fn start(
        &self,
        lease: WorkspaceLease,
    ) -> CapabilityFuture<'_, Result<CapabilityRuntimeHandle, CapabilityProviderError>> {
        Box::pin(async move {
            if (self.installation_probe)().state != CapabilityInstallationState::Installed {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::Unavailable,
                });
            }
            self.require_ready(&lease).await?;
            let key = CodeGraphRuntimeKey::from_lease(&lease);
            if self.runtimes.lock().await.contains_key(&key) {
                return Err(deferred_operation());
            }
            let runtime = (self.runtime_starter)(lease.clone()).await?;
            let mut runtimes = self.runtimes.lock().await;
            if runtimes.contains_key(&key) {
                return Err(deferred_operation());
            }
            runtimes.insert(key, runtime);
            Ok(CapabilityRuntimeHandle::new(
                self.descriptor.provider_id.clone(),
                &lease,
            ))
        })
    }

    /// 仅从 RuntimeSlot 交回的 handle 取得同 Workspace client；Tool payload 不可指定 root。
    fn call<'a>(
        &'a self,
        lease: &'a WorkspaceLease,
        runtime: Option<&'a CapabilityRuntimeHandle>,
        tool: WorkspaceToolCall,
    ) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>> {
        Box::pin(async move {
            let Some(runtime) = runtime else {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::RuntimeIdentityMismatch,
                });
            };
            if !runtime.matches_identity(
                &self.descriptor.provider_id,
                &lease.workspace_id,
                lease.generation,
            ) || tool.tool_name != "codegraph_explore"
            {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::ContractError,
                });
            }
            let arguments = codegraph_request(tool.arguments, lease)?;
            let key = CodeGraphRuntimeKey::from_lease(lease);
            let client = self
                .runtimes
                .lock()
                .await
                .get(&key)
                .cloned()
                .ok_or_else(deferred_operation)?;
            Ok(WorkspaceToolResult {
                result: Value::String(client.explore(arguments).await?),
            })
        })
    }

    /// 仅删除精确 identity 的私有 client；drop 会取消 transport 并由受管 child cleanup 收敛。
    fn stop(
        &self,
        runtime: CapabilityRuntimeHandle,
    ) -> CapabilityFuture<'_, Result<StopEvidence, CapabilityStopFailure>> {
        Box::pin(async move {
            let key = self
                .runtimes
                .lock()
                .await
                .keys()
                .find(|key| {
                    runtime.matches_identity(
                        &self.descriptor.provider_id,
                        &key.workspace_id,
                        key.generation,
                    )
                })
                .cloned();
            let Some(key) = key else {
                return Err(CapabilityStopFailure {
                    runtime,
                    error: deferred_operation(),
                });
            };
            let removed = self.runtimes.lock().await.remove(&key);
            if removed.is_none() {
                return Err(CapabilityStopFailure {
                    runtime,
                    error: deferred_operation(),
                });
            }
            Ok(StopEvidence {
                runtime_state: CapabilityRuntimeState::Stopped,
            })
        })
    }
}

/// 保持旧 CodeGraph query/maxFiles 语义，同时剥离 workspaceId 等路由字段。
fn codegraph_request(
    arguments: Value,
    lease: &WorkspaceLease,
) -> Result<Value, CapabilityProviderError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Args {
        query: String,
        #[serde(default)]
        max_files: Option<u32>,
        #[serde(default)]
        workspace_id: Option<String>,
    }
    let args: Args = serde_json::from_value(arguments).map_err(|_| deferred_operation())?;
    if args.query.trim().is_empty()
        || args
            .workspace_id
            .is_some_and(|workspace_id| workspace_id != lease.workspace_id)
    {
        return Err(deferred_operation());
    }
    Ok(json!({"query":args.query,"maxFiles":args.max_files.unwrap_or(12)}))
}

/// 收敛所有 Runtime 私有启动、transport 与 schema 失败。
fn deferred_operation() -> CapabilityProviderError {
    CapabilityProviderError {
        code: CapabilityProviderErrorCode::OperationFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_capability::{WorkspaceCapabilityManager, WorkspaceCapabilityRegistry};
    use std::{
        collections::VecDeque,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };
    use tokio::sync::{Notify, oneshot};

    /// 建立指向真实临时目录的 Lease，确保 projectPath canonical identity 可被测试验证。
    fn lease(root: PathBuf) -> WorkspaceLease {
        lease_with_identity("workspace-a", root, 7)
    }

    /// 构造不同 workspace/generation 的服务端 Lease，验证 Slot 只能按完整 identity 清理。
    fn lease_with_identity(workspace_id: &str, root: PathBuf, generation: u64) -> WorkspaceLease {
        WorkspaceLease {
            workspace_id: workspace_id.into(),
            canonical_root: root.canonicalize().unwrap(),
            generation,
        }
    }

    /// 生成最小未初始化状态 fixture。
    fn uninitialized(root: &std::path::Path) -> String {
        format!(
            r#"{{"initialized":false,"projectPath":{}}}"#,
            serde_json::to_string(root).unwrap()
        )
    }

    /// 生成已初始化的真实字段 fixture，并保持额外 CLI 字段无关。
    fn initialized(
        root: &std::path::Path,
        added: u64,
        reindex_recommended: bool,
        state: &str,
    ) -> String {
        format!(
            r#"{{"initialized":true,"projectPath":{},"pendingChanges":{{"added":{added},"modified":0,"removed":0}},"index":{{"reindexRecommended":{reindex_recommended},"state":{}}}}}"#,
            serde_json::to_string(root).unwrap(),
            serde_json::to_string(state).unwrap(),
        )
    }

    /// 使用可检查队列替代真实 CLI；每次 invocation 都保存精确 argv 与 current_dir。
    fn provider_with_responses(
        responses: impl IntoIterator<Item = Result<String, CapabilityProviderError>>,
    ) -> (
        CodeGraphCapabilityProvider,
        Arc<Mutex<Vec<CodeGraphCommand>>>,
    ) {
        let responses = Arc::new(Mutex::new(VecDeque::from_iter(responses)));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_runner = Arc::clone(&calls);
        let provider = CodeGraphCapabilityProvider::with_probes(
            Arc::new(|| CapabilityInstallation {
                state: CapabilityInstallationState::Installed,
                detected_version: Some("1.6.0".into()),
            }),
            Arc::new(move |command| {
                calls_for_runner.lock().unwrap().push(command);
                let response = responses.lock().unwrap().pop_front().unwrap();
                Box::pin(async move { response })
            }),
        );
        (provider, calls)
    }

    /// 缺 binary 必须只投影 not_installed，不能从目录或 status 猜测 ready。
    #[tokio::test]
    async fn missing_binary_projects_not_installed_without_running_status() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_runner = Arc::clone(&calls);
        let provider = CodeGraphCapabilityProvider::with_probes(
            Arc::new(|| CapabilityInstallation {
                state: CapabilityInstallationState::NotInstalled,
                detected_version: None,
            }),
            Arc::new(move |_| {
                calls_for_runner.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { panic!("missing binary must not run status") })
            }),
        );
        assert_eq!(
            provider.probe_installation().await.unwrap().state,
            CapabilityInstallationState::NotInstalled
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    /// 未初始化、ready 与两类 stale evidence 均只从实机 JSON 字段投影。
    #[tokio::test]
    async fn readiness_projects_uninitialized_ready_and_stale_statuses() {
        let directory = tempfile::tempdir().unwrap();
        let target = lease(directory.path().to_path_buf());
        let (provider, _) = provider_with_responses([
            Ok(uninitialized(&target.canonical_root)),
            Ok(initialized(&target.canonical_root, 0, false, "complete")),
            Ok(initialized(&target.canonical_root, 1, false, "complete")),
            Ok(initialized(&target.canonical_root, 0, true, "complete")),
        ]);
        for (expected_readiness, expected_stage) in [
            (
                CapabilityReadinessState::NotPrepared,
                CapabilityStageState::Absent,
            ),
            (CapabilityReadinessState::Ready, CapabilityStageState::Ready),
            (
                CapabilityReadinessState::Degraded,
                CapabilityStageState::Stale,
            ),
            (
                CapabilityReadinessState::Degraded,
                CapabilityStageState::Stale,
            ),
        ] {
            let observation = provider.observe_readiness(target.clone()).await.unwrap();
            assert_eq!(observation.readiness, expected_readiness);
            assert_eq!(observation.stages[0].state, expected_stage);
        }
    }

    /// 畸形 JSON、缺必需字段、命令错误与 canonical root mismatch 都不得产生 ready observation。
    #[tokio::test]
    async fn readiness_fails_closed_for_schema_command_and_root_errors() {
        let directory = tempfile::tempdir().unwrap();
        let target = lease(directory.path().to_path_buf());
        let other = tempfile::tempdir().unwrap();
        let failure = CapabilityProviderError {
            code: CapabilityProviderErrorCode::OperationFailed,
        };
        let missing_index_fields = format!(
            r#"{{"initialized":true,"projectPath":{}}}"#,
            serde_json::to_string(&target.canonical_root).unwrap()
        );
        let (provider, _) = provider_with_responses([
            Ok("not-json".into()),
            Ok(missing_index_fields),
            Err(failure.clone()),
            Ok(initialized(other.path(), 0, false, "complete")),
        ]);
        for _ in 0..4 {
            assert!(provider.observe_readiness(target.clone()).await.is_err());
        }
    }

    /// 三个动作严格冻结 CLI argv、cwd 和 post-action status；sync 成功后可合法保持 degraded。
    #[tokio::test]
    async fn explicit_actions_use_exact_argv_root_and_post_status() {
        let directory = tempfile::tempdir().unwrap();
        let target = lease(directory.path().to_path_buf());
        let ready = initialized(&target.canonical_root, 0, false, "complete");
        let stale = initialized(&target.canonical_root, 0, true, "complete");
        let (provider, calls) = provider_with_responses([
            Ok("done".into()),
            Ok(ready.clone()),
            Ok("done".into()),
            Ok(stale),
            Ok("done".into()),
            Ok(ready),
        ]);
        for (action_id, expected) in [
            ("build_index", CapabilityReadinessState::Ready),
            ("update_index", CapabilityReadinessState::Degraded),
            ("rebuild_index", CapabilityReadinessState::Ready),
        ] {
            assert_eq!(
                provider
                    .prepare(
                        target.clone(),
                        CapabilityPrepareAction {
                            action_id: action_id.into()
                        },
                        &NoopSink
                    )
                    .await
                    .unwrap()
                    .readiness,
                expected
            );
        }
        let calls = calls.lock().unwrap();
        let root = target.canonical_root.clone().into_os_string();
        assert_eq!(calls.len(), 6);
        for (pair, expected) in calls.chunks_exact(2).zip([
            ["init", "--yes"].as_slice(),
            ["sync"].as_slice(),
            ["index"].as_slice(),
        ]) {
            assert_eq!(pair[0].current_dir, target.canonical_root);
            assert_eq!(
                pair[0].args[..expected.len()],
                expected.iter().map(OsString::from).collect::<Vec<_>>()
            );
            assert_eq!(pair[0].args.last(), Some(&root));
            assert_eq!(pair[1], status_command(&target));
        }
    }

    /// action exit 成功但 post-status 仍为未初始化时必须失败，不能按 exit code 宣称完成。
    #[tokio::test]
    async fn explicit_action_requires_post_status_precondition() {
        let directory = tempfile::tempdir().unwrap();
        let target = lease(directory.path().to_path_buf());
        let (provider, _) =
            provider_with_responses([Ok("done".into()), Ok(uninitialized(&target.canonical_root))]);
        assert!(
            provider
                .prepare(
                    target,
                    CapabilityPrepareAction {
                        action_id: "build_index".into(),
                    },
                    &NoopSink,
                )
                .await
                .is_err()
        );
    }

    /// Health DTO 从 Registry descriptor 生成 CodeGraph stages/actions，不存在 providerId Core 特判。
    #[tokio::test]
    async fn health_projects_descriptor_actions_without_a_remote_tool() {
        let directory = tempfile::tempdir().unwrap();
        let target = lease(directory.path().to_path_buf());
        let (provider, _) = provider_with_responses([Ok(initialized(
            &target.canonical_root,
            0,
            false,
            "complete",
        ))]);
        let provider: Arc<dyn WorkspaceCapabilityProvider> = Arc::new(provider);
        let manager = WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new([provider]).unwrap(),
        ));
        let health = manager.observe_health(target).await;
        let codegraph = health.providers.get("codegraph").unwrap();
        assert_eq!(codegraph.readiness, CapabilityReadinessState::Ready);
        assert_eq!(codegraph.stages[0].id, "index");
        assert_eq!(codegraph.actions.len(), 3);
        assert!(
            codegraph
                .actions
                .iter()
                .all(|action| action.authority == CapabilityActionAuthority::LocalHuman)
        );
    }

    /// 同 action 使用既有 Manager flight，取消会丢弃唯一 Provider future 且不会建立 RuntimeSlot。
    #[tokio::test]
    async fn action_duplicates_share_manager_flight_and_cancel() {
        let directory = tempfile::tempdir().unwrap();
        let target = lease(directory.path().to_path_buf());
        let entered = Arc::new(Notify::new());
        let (_release, receiver) = oneshot::channel::<()>();
        let receiver = Arc::new(Mutex::new(Some(receiver)));
        let calls = Arc::new(AtomicUsize::new(0));
        let runner_entered = Arc::clone(&entered);
        let runner_receiver = Arc::clone(&receiver);
        let runner_calls = Arc::clone(&calls);
        let provider = Arc::new(CodeGraphCapabilityProvider::with_probes(
            Arc::new(|| CapabilityInstallation {
                state: CapabilityInstallationState::Installed,
                detected_version: None,
            }),
            Arc::new(move |_| {
                runner_calls.fetch_add(1, Ordering::SeqCst);
                let receiver = runner_receiver.lock().unwrap().take().unwrap();
                let entered = Arc::clone(&runner_entered);
                Box::pin(async move {
                    entered.notify_one();
                    let _ = receiver.await;
                    Ok("done".into())
                })
            }),
        ));
        let registered: Arc<dyn WorkspaceCapabilityProvider> = provider;
        let manager = Arc::new(WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new([registered]).unwrap(),
        )));
        let (activities, mut received) = tokio::sync::mpsc::unbounded_channel();
        let first = {
            let manager = Arc::clone(&manager);
            let target = target.clone();
            tokio::spawn(async move {
                manager
                    .prepare_action(
                        target,
                        "codegraph",
                        "build_index",
                        Arc::new(ActivitySink(activities)),
                    )
                    .await
            })
        };
        entered.notified().await;
        let running = received.recv().await.unwrap();
        assert_eq!(running.state, "running");
        let duplicate = {
            let manager = Arc::clone(&manager);
            let target = target.clone();
            tokio::spawn(async move {
                manager
                    .prepare_action(target, "codegraph", "build_index", Arc::new(NoopSink))
                    .await
            })
        };
        manager.cancel_action(&running.operation_id).unwrap();
        assert!(first.await.unwrap().is_err());
        assert!(duplicate.await.unwrap().is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// 空 sink 只用于直接 Provider/Manager 测试，不接收任何内部 command 信息。
    struct NoopSink;
    impl CapabilityActivitySink for NoopSink {}

    /// 测试仅转交 Manager 已净化的活动事件，以取得 opaque operationId 触发正式取消路径。
    struct ActivitySink(
        tokio::sync::mpsc::UnboundedSender<crate::workspace_capability::CapabilityActivity>,
    );
    impl CapabilityActivitySink for ActivitySink {
        fn publish<'a>(
            &'a self,
            activity: crate::workspace_capability::CapabilityActivity,
        ) -> CapabilityFuture<'a, ()> {
            Box::pin(async move {
                self.0.send(activity).unwrap();
            })
        }
    }

    /// 测试 Runtime 的析构即代表 Provider 仍由目标 Slot 持有并已精确交回 stop/drop 路径。
    struct DropProbeClient {
        drops: Arc<AtomicUsize>,
    }

    impl Drop for DropProbeClient {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl CodeGraphRuntimeClient for DropProbeClient {
        fn explore<'a>(
            &'a self,
            _arguments: Value,
        ) -> CapabilityFuture<'a, Result<String, CapabilityProviderError>> {
            Box::pin(async { Ok("ok".into()) })
        }
    }

    /// 用 channel 固定一次尚未完成的 query，验证 shutdown 先等待 Manager guard 再释放 child ownership。
    struct BlockingDropProbeClient {
        drops: Arc<AtomicUsize>,
        entered: Arc<Notify>,
        release: Mutex<Option<oneshot::Receiver<()>>>,
    }

    impl Drop for BlockingDropProbeClient {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl CodeGraphRuntimeClient for BlockingDropProbeClient {
        fn explore<'a>(
            &'a self,
            _arguments: Value,
        ) -> CapabilityFuture<'a, Result<String, CapabilityProviderError>> {
            let release = self.release.lock().unwrap().take().unwrap();
            let entered = Arc::clone(&self.entered);
            Box::pin(async move {
                entered.notify_one();
                let _ = release.await;
                Ok("ok".into())
            })
        }
    }

    /// 构造始终通过 status readiness 的受控 Provider，Runtime 本身完全由测试工厂持有。
    fn ready_runtime_provider(runtime_starter: RuntimeStarter) -> CodeGraphCapabilityProvider {
        CodeGraphCapabilityProvider::with_runtime(
            Arc::new(|| CapabilityInstallation {
                state: CapabilityInstallationState::Installed,
                detected_version: Some("1.6.0".into()),
            }),
            Arc::new(|command| {
                let output = initialized(&command.current_dir, 0, false, "complete");
                Box::pin(async move { Ok(output) })
            }),
            runtime_starter,
        )
    }

    /// 将单个 CodeGraph Provider 接入真实 Manager stop/remove/shutdown 路径。
    fn codegraph_manager(provider: CodeGraphCapabilityProvider) -> Arc<WorkspaceCapabilityManager> {
        let provider: Arc<dyn WorkspaceCapabilityProvider> = Arc::new(provider);
        Arc::new(WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new([provider]).unwrap(),
        )))
    }

    #[tokio::test]
    /// A Remove 只 drop A 的 direct-client ownership；B 持续可用，最终 shutdown 收敛两者。
    async fn remove_and_shutdown_drop_only_the_targeted_codegraph_slots() {
        let directory_a = tempfile::tempdir().unwrap();
        let directory_b = tempfile::tempdir().unwrap();
        let workspace_a = lease_with_identity("workspace-a", directory_a.path().into(), 7);
        let workspace_b = lease_with_identity("workspace-b", directory_b.path().into(), 7);
        let drops = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Mutex::new(Vec::new()));
        let provider = ready_runtime_provider(Arc::new({
            let drops = Arc::clone(&drops);
            let started = Arc::clone(&started);
            move |lease| {
                let drops = Arc::clone(&drops);
                let started = Arc::clone(&started);
                Box::pin(async move {
                    started.lock().unwrap().push(lease.workspace_id);
                    Ok(Arc::new(DropProbeClient { drops }) as Arc<dyn CodeGraphRuntimeClient>)
                })
            }
        }));
        let manager = codegraph_manager(provider);

        drop(
            manager
                .acquire_runtime("codegraph", workspace_a.clone())
                .await
                .unwrap(),
        );
        drop(
            manager
                .acquire_runtime("codegraph", workspace_b.clone())
                .await
                .unwrap(),
        );
        let removal = manager.begin_workspace_remove(&workspace_a).await.unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(
            started.lock().unwrap().as_slice(),
            ["workspace-a", "workspace-b"]
        );

        drop(
            manager
                .acquire_runtime("codegraph", workspace_b.clone())
                .await
                .unwrap(),
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1, "A Remove must not drop B");
        drop(removal);
        manager.shutdown_runtimes().await.unwrap();
        assert_eq!(
            drops.load(Ordering::SeqCst),
            2,
            "shutdown drops the remaining B client"
        );
    }

    #[tokio::test]
    /// generation drift 只能停止旧 Slot；同 workspace ID 的新 generation 不会被 Remove 误伤。
    async fn remove_keeps_newer_generation_codegraph_slot_owned_until_shutdown() {
        let directory = tempfile::tempdir().unwrap();
        let old = lease_with_identity("workspace", directory.path().into(), 7);
        let newer = lease_with_identity("workspace", directory.path().into(), 8);
        let drops = Arc::new(AtomicUsize::new(0));
        let provider = ready_runtime_provider(Arc::new({
            let drops = Arc::clone(&drops);
            move |_lease| {
                let drops = Arc::clone(&drops);
                Box::pin(async move {
                    Ok(Arc::new(DropProbeClient { drops }) as Arc<dyn CodeGraphRuntimeClient>)
                })
            }
        }));
        let manager = codegraph_manager(provider);

        drop(
            manager
                .acquire_runtime("codegraph", old.clone())
                .await
                .unwrap(),
        );
        drop(
            manager
                .acquire_runtime("codegraph", newer.clone())
                .await
                .unwrap(),
        );
        let removal = manager.begin_workspace_remove(&old).await.unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(manager.acquire_runtime("codegraph", newer).await.unwrap());
        drop(removal);
        manager.shutdown_runtimes().await.unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    /// 正在执行的 CodeGraph call 会持有 Slot；shutdown 不可抢先 drop direct child ownership。
    async fn shutdown_waits_for_inflight_codegraph_call_before_dropping_client() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = lease_with_identity("workspace", directory.path().into(), 7);
        let drops = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Notify::new());
        let (release_tx, release_rx) = oneshot::channel();
        let release_rx = Arc::new(Mutex::new(Some(release_rx)));
        let provider = ready_runtime_provider(Arc::new({
            let drops = Arc::clone(&drops);
            let entered = Arc::clone(&entered);
            let release_rx = Arc::clone(&release_rx);
            move |_lease| {
                let drops = Arc::clone(&drops);
                let entered = Arc::clone(&entered);
                let release = Mutex::new(release_rx.lock().unwrap().take());
                Box::pin(async move {
                    Ok(Arc::new(BlockingDropProbeClient {
                        drops,
                        entered,
                        release,
                    }) as Arc<dyn CodeGraphRuntimeClient>)
                })
            }
        }));
        let manager = codegraph_manager(provider);
        let call_manager = Arc::clone(&manager);
        let call_workspace = workspace.clone();
        let call = tokio::spawn(async move {
            call_manager
                .call(
                    "codegraph",
                    call_workspace,
                    WorkspaceToolCall {
                        tool_name: "codegraph_explore".into(),
                        arguments: json!({"query":"ownership"}),
                        cancellation: CancellationToken::new(),
                    },
                )
                .await
        });

        entered.notified().await;
        let shutdown_admitted = manager.shutdown_admission_notifier();
        let shutdown_admitted = shutdown_admitted.notified();
        let shutdown_manager = Arc::clone(&manager);
        let shutdown = tokio::spawn(async move { shutdown_manager.shutdown_runtimes().await });
        shutdown_admitted.await;
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "in-flight call must retain direct-client ownership until it returns"
        );
        release_tx.send(()).unwrap();
        call.await.unwrap().unwrap();
        shutdown.await.unwrap().unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    /// Descriptor 在 Registry 内声明 Runtime tool ownership；P2D-009 只在 MCP registry 恢复公开 route。
    #[test]
    fn descriptor_keeps_runtime_contract_and_actions_generic() {
        let provider = CodeGraphCapabilityProvider::new();
        let descriptor = provider.descriptor();
        assert_eq!(descriptor.provider_id.as_str(), "codegraph");
        assert_eq!(descriptor.tool_names, ["codegraph_explore"]);
        assert_eq!(
            descriptor.runtime_model,
            CapabilityRuntimeModel::WorkspaceScopedProcess
        );
        assert_eq!(descriptor.runtime_policy.max_instances, 2);
        assert_eq!(descriptor.runtime_policy.idle_timeout_ms, 300_000);
        assert_eq!(descriptor.runtime_policy.per_slot_concurrency, 1);
        assert_eq!(descriptor.action_descriptors.len(), 3);
        for action in &descriptor.action_descriptors {
            assert_eq!(action.authority, CapabilityActionAuthority::LocalHuman);
            assert_eq!(action.execution, CapabilityActionExecution::ProviderPrepare);
            assert!(!action.warm_runtime);
        }
    }
}

//! Serena optional semantic capability 的无副作用 adapter shell。

use crate::{
    config::{self, AppPaths, ManagerConfig},
    discovery::{self, InstallationState, SerenaInstallation},
    mcp::serena::Client,
    serena::{contain_process, hidden_command, terminate_managed_job, terminate_managed_process},
    workspace_capability::{
        CapabilityAction, CapabilityActionAuthority, CapabilityActionDescriptor,
        CapabilityActionExecution, CapabilityActivitySink, CapabilityFuture,
        CapabilityInstallation, CapabilityInstallationState, CapabilityPrepareAction,
        CapabilityPrepareResult, CapabilityProviderError, CapabilityProviderErrorCode,
        CapabilityReadinessProbe, CapabilityReadinessState, CapabilityRuntimeHandle,
        CapabilityRuntimeModel, CapabilityRuntimePolicy, CapabilityRuntimeState, CapabilityStage,
        CapabilityStageDescriptor, CapabilityStageRequirement, CapabilityStageState,
        CapabilityStopFailure, StopEvidence, WorkspaceCapabilityDescriptor,
        WorkspaceCapabilityProvider, WorkspaceCapabilityProviderId, WorkspaceToolCall,
        WorkspaceToolResult,
    },
    workspace_resolver::WorkspaceLease,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

#[cfg(windows)]
#[path = "serena_capability_index_windows.rs"]
mod index_windows;
#[cfg(windows)]
use index_windows::IndexProcess;

/// P0-005 已验证的 Serena 首版跨 Workspace 并发容量。
const SERENA_RUNTIME_MAX_INSTANCES: usize = 2;
/// 每个独立 Serena Slot 的首版串行调用上限。
const SERENA_SHELL_PER_SLOT_CONCURRENCY: usize = 1;
/// 避免约 2.6～2.8 秒启动开销造成 churn 的保守内部 timeout。
const SERENA_RUNTIME_IDLE_TIMEOUT_MS: u64 = 60_000;

/// 隔离 CLI 探测的 fixture seam，生产实现仍直接复用 discovery::detect。
type InstallationDetector = Arc<dyn Fn() -> SerenaInstallation + Send + Sync>;
/// 隔离 project.yml 文件检查的 fixture seam，错误不会携带路径或原始文件系统信息。
type ProjectConfigurationProbe =
    Arc<dyn Fn(&Path) -> Result<bool, CapabilityProviderError> + Send + Sync>;

/// 单次受管 index 的 deterministic seam；不创建新的长期 Runtime。
type IndexRunner = Arc<
    dyn Fn(Command) -> CapabilityFuture<'static, Result<(), CapabilityProviderError>> + Send + Sync,
>;

/// Provider-private Runtime key；不形成 Manager handle 或公开数据传输对象。
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct SerenaRuntimeKey {
    workspace_id: String,
    generation: u64,
}

impl SerenaRuntimeKey {
    /// 从已解析 Lease 生成 slot identity，绝不从 Serena projects registry 推断。
    fn from_lease(lease: &WorkspaceLease) -> Self {
        Self {
            workspace_id: lease.workspace_id.clone(),
            generation: lease.generation,
        }
    }
}

/// Serena Runtime 内部 Client port；只用于在释放 Runtime map 锁后持有目标 Slot 的调用引用。
trait SerenaRuntimeClient: Send + Sync {
    /// 复用既有 Client cancellation 行为，并将 transport 原始错误收敛为 Provider 安全错误。
    fn call<'a>(
        &'a self,
        name: &'a str,
        arguments: serde_json::Value,
    ) -> CapabilityFuture<'a, Result<String, CapabilityProviderError>>;
}

impl SerenaRuntimeClient for Client {
    /// 生产 Runtime 继续直接调用既有 Serena Client，不引入第二套取消机制。
    fn call<'a>(
        &'a self,
        name: &'a str,
        arguments: serde_json::Value,
    ) -> CapabilityFuture<'a, Result<String, CapabilityProviderError>> {
        Box::pin(async move {
            Client::call(self, name, arguments)
                .await
                .map_err(|_| deferred_operation())
        })
    }
}

/// Provider-private 进程与 Client 所有权；Manager 只持有 opaque handle。
struct SerenaRuntime {
    client: Arc<dyn SerenaRuntimeClient>,
    child: Child,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
}

/// Serena Capability Provider 负责独立 Slot 的 process/client 所有权。
pub(crate) struct SerenaCapabilityProvider {
    descriptor: WorkspaceCapabilityDescriptor,
    installation_detector: InstallationDetector,
    project_configuration_probe: ProjectConfigurationProbe,
    runtime_directory: PathBuf,
    runtimes: Mutex<HashMap<SerenaRuntimeKey, SerenaRuntime>>,
    index_runner: IndexRunner,
}

impl SerenaCapabilityProvider {
    /// 以实时配置快照和既有 discovery 逻辑构造生产 Provider；不探测、不启动、不写文件。
    pub(crate) fn new(
        config: Arc<dyn Fn() -> ManagerConfig + Send + Sync>,
        paths: AppPaths,
    ) -> Self {
        let detector_config = Arc::clone(&config);
        let detector_paths = paths.clone();
        Self::with_probes(
            Arc::new(move || discovery::detect(&(detector_config)(), &detector_paths)),
            Arc::new(project_configuration_exists),
            paths.runtime_directory,
        )
    }

    /// 以 deterministic probe fixture 构造 Provider，仅供本模块测试验证边界。
    fn with_probes(
        installation_detector: InstallationDetector,
        project_configuration_probe: ProjectConfigurationProbe,
        runtime_directory: PathBuf,
    ) -> Self {
        Self {
            descriptor: WorkspaceCapabilityDescriptor {
                provider_id: WorkspaceCapabilityProviderId::new("serena"),
                display_name: "Serena".into(),
                // Phase 2B Rust Source cutover 前，基础 Source 与 Semantic Source 共用 Serena Slot ownership。
                tool_names: vec![
                    "source_read_file".into(),
                    "source_list_dir".into(),
                    "source_find_file".into(),
                    "source_search_pattern".into(),
                    "source_symbols_overview".into(),
                    "source_find_symbol".into(),
                    "source_find_references".into(),
                ],
                runtime_model: CapabilityRuntimeModel::WorkspaceScopedProcess,
                readiness_probe: CapabilityReadinessProbe::Required,
                preparation_policy:
                    crate::workspace_capability::CapabilityPreparationPolicy::AutoOnFirstToolCall,
                stage_descriptors: vec![
                    CapabilityStageDescriptor {
                        id: "project_configuration".into(),
                        display_name: "项目配置".into(),
                        requirement: CapabilityStageRequirement::AutoPreparable,
                    },
                    CapabilityStageDescriptor {
                        id: "index".into(),
                        display_name: "符号索引".into(),
                        requirement: CapabilityStageRequirement::Optional,
                    },
                    CapabilityStageDescriptor {
                        id: "onboarding".into(),
                        display_name: "项目认知".into(),
                        requirement: CapabilityStageRequirement::Optional,
                    },
                ],
                action_descriptors: vec![
                    CapabilityActionDescriptor {
                        action_id: "prepare".into(),
                        display_name: "准备".into(),
                        authority: CapabilityActionAuthority::LocalHuman,
                        execution: CapabilityActionExecution::ManagerEnsureRuntime,
                        warm_runtime: true,
                    },
                    CapabilityActionDescriptor {
                        action_id: "build_index".into(),
                        display_name: "建立索引".into(),
                        authority: CapabilityActionAuthority::LocalHuman,
                        execution: CapabilityActionExecution::ProviderPrepare,
                        warm_runtime: false,
                    },
                ],
                runtime_policy: CapabilityRuntimePolicy {
                    max_instances: SERENA_RUNTIME_MAX_INSTANCES,
                    idle_timeout_ms: SERENA_RUNTIME_IDLE_TIMEOUT_MS,
                    per_slot_concurrency: SERENA_SHELL_PER_SLOT_CONCURRENCY,
                },
            },
            installation_detector,
            project_configuration_probe,
            runtime_directory,
            runtimes: Mutex::new(HashMap::new()),
            index_runner: Arc::new(run_index),
        }
    }

    /// 将 discovery 的既有安装/版本策略映射到冻结 CapabilityInstallation 安全外壳。
    fn installation(&self) -> CapabilityInstallation {
        let detected = (self.installation_detector)();
        match detected.state {
            InstallationState::Standard => CapabilityInstallation {
                state: CapabilityInstallationState::Installed,
                detected_version: Some(detected.version),
            },
            InstallationState::Missing => CapabilityInstallation {
                state: CapabilityInstallationState::NotInstalled,
                detected_version: None,
            },
            // discovery 会将显式损坏路径归为 Invalid；Capability 契约仍将实际缺失 binary 投影为 NotInstalled。
            InstallationState::Invalid if !detected.path.is_file() => CapabilityInstallation {
                state: CapabilityInstallationState::NotInstalled,
                detected_version: None,
            },
            // CLI 失败、不可解析、fork 或不兼容版本均沿用 discovery 的 Invalid 策略，且不暴露原始原因。
            InstallationState::Invalid => CapabilityInstallation {
                state: CapabilityInstallationState::CheckFailed,
                detected_version: None,
            },
        }
    }

    /// 生成仅含冻结通用字段的 Serena stage 投影，绝不从 project.yml 推断 Index/Onboarding。
    fn stages(&self, project_configuration: CapabilityStageState) -> Vec<CapabilityStage> {
        vec![
            CapabilityStage {
                id: "project_configuration".into(),
                display_name: "项目配置".into(),
                state: project_configuration,
                requirement: CapabilityStageRequirement::AutoPreparable,
                message_code: (project_configuration == CapabilityStageState::Absent)
                    .then(|| "CAPABILITY_STAGE_NOT_PREPARED".into()),
            },
            CapabilityStage {
                id: "index".into(),
                display_name: "符号索引".into(),
                state: CapabilityStageState::Unknown,
                requirement: CapabilityStageRequirement::Optional,
                message_code: None,
            },
            CapabilityStage {
                id: "onboarding".into(),
                display_name: "项目认知".into(),
                state: CapabilityStageState::Unknown,
                requirement: CapabilityStageRequirement::Optional,
                message_code: None,
            },
        ]
    }

    /// 将已声明动作投影到统一 observation，007 不会执行其中任何一个动作。
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

    /// 构造安装不可用或检查失败时的 fail-closed observation，避免读取 Workspace 文件。
    fn unavailable_observation(
        &self,
        installation: CapabilityInstallation,
    ) -> crate::workspace_capability::CapabilityObservation {
        crate::workspace_capability::CapabilityObservation {
            provider_id: self.descriptor.provider_id.clone(),
            installation: installation.state,
            readiness: CapabilityReadinessState::Unknown,
            runtime_state: CapabilityRuntimeState::Stopped,
            checked_at: checked_at(),
            stages: self.stages(CapabilityStageState::Unknown),
            actions: self.actions(),
        }
    }

    /// 为单个 Slot 生成稳定、受限且不携带 legacy workspaceId 的受管 Home 路径。
    fn slot_home(&self, lease: &WorkspaceLease) -> PathBuf {
        let mut hash = Sha256::new();
        hash.update(b"serena\0");
        hash.update(lease.workspace_id.as_bytes());
        hash.update(b"\0");
        hash.update(lease.generation.to_le_bytes());
        let digest = hash
            .finalize()
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        self.runtime_directory
            .join("serena-slots")
            .join(format!("serena-{digest}-g{}", lease.generation))
    }

    /// Slot context 与其 Home 同属受管 runtime 目录，且不复用 Broker context。
    fn slot_context(&self, lease: &WorkspaceLease) -> PathBuf {
        self.slot_home(lease).join("workspace-context.yml")
    }

    /// 选择短生命周期 loopback port；启动后仍由 Client 连通性验证最终 endpoint。
    fn select_loopback_port() -> Result<u16, CapabilityProviderError> {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|listener| listener.local_addr())
            .map(|address| address.port())
            .map_err(|_| deferred_operation())
    }

    /// 将已经创建的 child 收敛，失败时不向 Capability 边界泄露进程细节。
    async fn cleanup_child(child: &mut Child) {
        let _ = terminate_managed_process(child);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    /// 启动一条严格绑定单个 Lease 的 Serena process，并在任一失败路径回收 child。
    async fn start_runtime(
        &self,
        lease: &WorkspaceLease,
        installation: SerenaInstallation,
        home: &Path,
        context: &Path,
        port: u16,
    ) -> Result<SerenaRuntime, CapabilityProviderError> {
        let mut command = hidden_command(&installation.path);
        command
            .args(["start-mcp-server", "--project"])
            .arg(&lease.canonical_root)
            .args(["--context"])
            .arg(context)
            .args([
                "--transport",
                "streamable-http",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--open-web-dashboard",
                "false",
            ])
            .env("SERENA_HOME", home)
            .env("FASTMCP_JSON_RESPONSE", "false")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| deferred_operation())?;
        #[cfg(windows)]
        let job = match contain_process(&child) {
            Ok(job) => job,
            Err(_) => {
                Self::cleanup_child(&mut child).await;
                return Err(deferred_operation());
            }
        };
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return Err(deferred_operation()),
                Ok(None) => {}
                Err(_) => {
                    Self::cleanup_child(&mut child).await;
                    return Err(deferred_operation());
                }
            }
            if std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
                Duration::from_millis(120),
            )
            .is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                Self::cleanup_child(&mut child).await;
                return Err(deferred_operation());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // `--project` 只允许 Serena 创建 Project Configuration；在发布 opaque Runtime 前，
        // 必须重新以同一 Lease canonical root 确认其确实成为常规文件。
        match (self.project_configuration_probe)(&lease.canonical_root) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                Self::cleanup_child(&mut child).await;
                return Err(deferred_operation());
            }
        }
        let client = match Client::connect(port).await {
            Ok(client) => client,
            Err(_) => {
                Self::cleanup_child(&mut child).await;
                return Err(deferred_operation());
            }
        };
        if client.activate(&lease.canonical_root).await.is_err() {
            Self::cleanup_child(&mut child).await;
            return Err(deferred_operation());
        }
        Ok(SerenaRuntime {
            client: Arc::new(client),
            child,
            #[cfg(windows)]
            job,
        })
    }

    /// 将全部七个 Source public name 转换为既有 Serena upstream 调用参数。
    ///
    /// 前四项仅是 Phase 2B Rust Source cutover 前的临时兼容 ownership，
    /// 不形成第二个 Provider、Runtime 或 Client 架构。
    fn source_request(
        tool: WorkspaceToolCall,
    ) -> Result<(&'static str, serde_json::Value), CapabilityProviderError> {
        let (upstream_name, default_limit, hard_limit, inject_max_answer_chars) =
            match tool.tool_name.as_str() {
                "source_read_file" => ("read_file", 32_768, 131_072, true),
                "source_list_dir" => ("list_dir", 65_536, 262_144, true),
                "source_find_file" => ("find_file", 65_536, 262_144, false),
                "source_search_pattern" => ("search_for_pattern", 65_536, 262_144, true),
                "source_symbols_overview" => ("get_symbols_overview", 65_536, 262_144, true),
                "source_find_symbol" => ("find_symbol", 65_536, 262_144, true),
                "source_find_references" => ("find_referencing_symbols", 65_536, 262_144, true),
                _ => return Err(deferred_operation()),
            };
        let mut arguments = tool
            .arguments
            .as_object()
            .cloned()
            .ok_or_else(deferred_operation)?;
        // workspaceId 是上层 Resolver 的 Authority 输入，绝不能进入 Serena upstream 参数。
        arguments.remove("workspaceId");
        // SourceArgs 的 deny_unknown_fields 同时拒绝 caller root/absolute root 等未冻结参数。
        let source_args: crate::mcp::registry::SourceArgs =
            serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                .map_err(|_| deferred_operation())?;
        let limit = source_args.max_bytes.unwrap_or(default_limit);
        if !(1..=hard_limit).contains(&limit) {
            return Err(deferred_operation());
        }
        let relative_path = source_args.relative_path.unwrap_or_default();
        if Path::new(&relative_path).is_absolute()
            || relative_path.contains(':')
            || Path::new(&relative_path).components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(deferred_operation());
        }
        arguments.remove("max_bytes");
        arguments.retain(|_, value| !value.is_null());
        arguments.insert(
            "relative_path".into(),
            serde_json::Value::String(relative_path),
        );
        if inject_max_answer_chars {
            arguments.insert("max_answer_chars".into(), serde_json::json!(limit));
        }
        if tool.tool_name == "source_list_dir" {
            arguments
                .entry("recursive")
                .or_insert(serde_json::json!(false));
        }
        Ok((upstream_name, serde_json::Value::Object(arguments)))
    }
}

/// 只检查 Lease canonical root 下的 Serena Project Configuration，不读取其内容也不创建路径。
fn project_configuration_exists(canonical_root: &Path) -> Result<bool, CapabilityProviderError> {
    match std::fs::metadata(canonical_root.join(".serena").join("project.yml")) {
        // 只有常规文件才是可用的 Serena Project Configuration；目录、FIFO 等一律视为未准备。
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        // 文件系统元数据错误只保留稳定 Provider 代码，避免错误中泄露本地路径。
        Err(_) => Err(CapabilityProviderError {
            code: CapabilityProviderErrorCode::OperationFailed,
        }),
    }
}

/// 返回 observation freshness 时间戳；时间源不参与任何 Workspace authority 判断。
fn checked_at() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// 007 尚未实现 Runtime/Tool/Preparation 时的统一 fail-closed Provider 错误。
fn deferred_operation() -> CapabilityProviderError {
    CapabilityProviderError {
        code: CapabilityProviderErrorCode::OperationFailed,
    }
}

/// Unix index 自有进程组；只属于本次操作，不建立长期 Runtime。
#[cfg(unix)]
struct IndexProcess {
    child: tokio::process::Child,
    process_group: i32,
}

#[cfg(unix)]
impl Drop for IndexProcess {
    fn drop(&mut self) {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        // SAFETY: process_group(0) 令 child PID 成为独占 PGID；负 PGID 只终止该组。
        unsafe {
            kill(-self.process_group, 9);
        }
        // tokio Child 的 kill_on_drop/reaper 继续负责直接 child 的回收。
    }
}

#[cfg(unix)]
impl IndexProcess {
    /// Command 每次只用于本次 index，并在 exec 前建立独立进程组。
    fn spawn(mut command: Command) -> std::io::Result<Self> {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        let child = tokio::process::Command::from(command)
            .kill_on_drop(true)
            .spawn()?;
        let process_group = child.id().expect("new index child must have a PID") as i32;
        Ok(Self {
            child,
            process_group,
        })
    }

    /// 状态仅用于当前 operation 结果，不形成 Health authority。
    fn try_wait(&mut self) -> std::io::Result<Option<bool>> {
        self.child
            .try_wait()
            .map(|status| status.map(|status| status.success()))
    }
}

/// 不捕获 stdout/stderr；控制逻辑只检查退出状态，并提供固定执行上界。
fn run_index(command: Command) -> CapabilityFuture<'static, Result<(), CapabilityProviderError>> {
    Box::pin(async move {
        let mut process = IndexProcess::spawn(command).map_err(|_| deferred_operation())?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
        loop {
            match process.try_wait().map_err(|_| deferred_operation())? {
                Some(success) => {
                    return if success {
                        Ok(())
                    } else {
                        Err(deferred_operation())
                    };
                }
                None if tokio::time::Instant::now() >= deadline => return Err(deferred_operation()),
                None => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    })
}

impl WorkspaceCapabilityProvider for SerenaCapabilityProvider {
    /// 返回 immutable Serena descriptor，不会触发 CLI 或文件系统访问。
    fn descriptor(&self) -> &WorkspaceCapabilityDescriptor {
        &self.descriptor
    }

    /// 调用既有 discovery/version policy，不启动 Runtime 或修改 Workspace。
    fn probe_installation(
        &self,
    ) -> CapabilityFuture<'_, Result<CapabilityInstallation, CapabilityProviderError>> {
        let installation = self.installation();
        Box::pin(async move { Ok(installation) })
    }

    /// 仅以 server-resolved Lease canonical root 检查 project.yml 存在性。
    fn observe_readiness(
        &self,
        lease: WorkspaceLease,
    ) -> CapabilityFuture<
        '_,
        Result<crate::workspace_capability::CapabilityObservation, CapabilityProviderError>,
    > {
        let installation = self.installation();
        if installation.state != CapabilityInstallationState::Installed {
            let observation = self.unavailable_observation(installation);
            return Box::pin(async move { Ok(observation) });
        }
        let provider_id = self.descriptor.provider_id.clone();
        let actions = self.actions();
        let project_configuration_probe = Arc::clone(&self.project_configuration_probe);
        Box::pin(async move {
            // Workspace authority 完全来自 Lease；不读取 Desktop selection、caller payload 或 project.yml 内容。
            let exists = project_configuration_probe(&lease.canonical_root)?;
            let stage_state = if exists {
                CapabilityStageState::Ready
            } else {
                CapabilityStageState::Absent
            };
            Ok(crate::workspace_capability::CapabilityObservation {
                provider_id,
                installation: installation.state,
                readiness: if exists {
                    CapabilityReadinessState::Ready
                } else {
                    CapabilityReadinessState::NotPrepared
                },
                runtime_state: CapabilityRuntimeState::Stopped,
                checked_at: checked_at(),
                stages: vec![
                    CapabilityStage {
                        id: "project_configuration".into(),
                        display_name: "项目配置".into(),
                        state: stage_state,
                        requirement: CapabilityStageRequirement::AutoPreparable,
                        message_code: (!exists).then(|| "CAPABILITY_STAGE_NOT_PREPARED".into()),
                    },
                    CapabilityStage {
                        id: "index".into(),
                        display_name: "符号索引".into(),
                        state: CapabilityStageState::Unknown,
                        requirement: CapabilityStageRequirement::Optional,
                        message_code: None,
                    },
                    CapabilityStage {
                        id: "onboarding".into(),
                        display_name: "项目认知".into(),
                        state: CapabilityStageState::Unknown,
                        requirement: CapabilityStageRequirement::Optional,
                        message_code: None,
                    },
                ],
                actions,
            })
        })
    }

    /// 只执行 Descriptor 声明的显式 index；配置缺失时绝不借 index 隐式创建。
    fn prepare<'a>(
        &'a self,
        lease: WorkspaceLease,
        action: CapabilityPrepareAction,
        _activity: &'a dyn CapabilityActivitySink,
    ) -> CapabilityFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>> {
        Box::pin(async move {
            if action.action_id != "build_index" {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::ContractError,
                });
            }
            let installation = (self.installation_detector)();
            if installation.state != InstallationState::Standard {
                return Err(deferred_operation());
            }
            if !(self.project_configuration_probe)(&lease.canonical_root)? {
                return Err(deferred_operation());
            }
            let home = self.slot_home(&lease);
            let context = self.slot_context(&lease);
            config::prepare_workspace_serena_home(&home, &context)
                .map_err(|_| deferred_operation())?;
            config::verify_workspace_serena_home(&home, &context)
                .map_err(|_| deferred_operation())?;
            let mut command = hidden_command(&installation.path);
            command
                .args(["project", "index"])
                .arg(&lease.canonical_root)
                .env("SERENA_HOME", &home)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            (self.index_runner)(command).await?;
            // 仅证明本次显式操作成功，不更新任何持久 Index Health authority。
            Ok(CapabilityPrepareResult {
                readiness: CapabilityReadinessState::Ready,
            })
        })
    }

    /// 为 Lease 启动独立 Serena Slot；缺失的 Project Configuration 仅由官方 --project 路径创建。
    fn start(
        &self,
        lease: WorkspaceLease,
    ) -> CapabilityFuture<'_, Result<CapabilityRuntimeHandle, CapabilityProviderError>> {
        Box::pin(async move {
            // probe 错误必须在 Slot Home/child 前 fail closed；仅明确 absent 才允许 --project 自动创建。
            (self.project_configuration_probe)(&lease.canonical_root)?;
            let installation = (self.installation_detector)();
            if installation.state != InstallationState::Standard {
                return Err(deferred_operation());
            }
            let key = SerenaRuntimeKey::from_lease(&lease);
            if self.runtimes.lock().await.contains_key(&key) {
                return Err(deferred_operation());
            }
            let home = self.slot_home(&lease);
            let context = self.slot_context(&lease);
            config::prepare_workspace_serena_home(&home, &context)
                .map_err(|_| deferred_operation())?;
            config::verify_workspace_serena_home(&home, &context)
                .map_err(|_| deferred_operation())?;
            let port = Self::select_loopback_port()?;
            let runtime = self
                .start_runtime(&lease, installation, &home, &context, port)
                .await?;
            let mut runtimes = self.runtimes.lock().await;
            if runtimes.contains_key(&key) {
                drop(runtimes);
                let mut runtime = runtime;
                Self::cleanup_child(&mut runtime.child).await;
                return Err(deferred_operation());
            }
            runtimes.insert(key, runtime);
            Ok(CapabilityRuntimeHandle::new(
                self.descriptor.provider_id.clone(),
                &lease,
            ))
        })
    }

    /// 仅在 Runtime identity 与 server-resolved Lease 完全一致时调用 Provider-private Client。
    fn call<'a>(
        &'a self,
        lease: &'a WorkspaceLease,
        runtime: Option<&'a CapabilityRuntimeHandle>,
        tool: WorkspaceToolCall,
    ) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>> {
        Box::pin(async move {
            let provider_id = &self.descriptor.provider_id;
            let Some(runtime) = runtime else {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::RuntimeIdentityMismatch,
                });
            };
            if !runtime.matches_identity(provider_id, &lease.workspace_id, lease.generation) {
                return Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::RuntimeIdentityMismatch,
                });
            }
            let key = SerenaRuntimeKey::from_lease(lease);
            // 只在短临界区内按 Lease identity 定位 Client；await 期间不持有全局 Runtime map 锁。
            let client = {
                let runtimes = self.runtimes.lock().await;
                let runtime = runtimes.get(&key).ok_or_else(deferred_operation)?;
                Arc::clone(&runtime.client)
            };
            let (upstream_name, arguments) = Self::source_request(tool)?;
            let text = client.call(upstream_name, arguments).await?;
            Ok(WorkspaceToolResult {
                result: serde_json::Value::String(text),
            })
        })
    }

    /// 精确停止 Provider-private Runtime；任何失败都返还输入的同一 opaque handle。
    fn stop(
        &self,
        runtime: CapabilityRuntimeHandle,
    ) -> CapabilityFuture<'_, Result<StopEvidence, CapabilityStopFailure>> {
        Box::pin(async move {
            let provider_id = &self.descriptor.provider_id;
            let mut runtimes = self.runtimes.lock().await;
            let key = runtimes
                .keys()
                .find(|key| {
                    runtime.matches_identity(provider_id, &key.workspace_id, key.generation)
                })
                .cloned();
            let Some(key) = key else {
                return Err(CapabilityStopFailure {
                    runtime,
                    error: deferred_operation(),
                });
            };
            let Some(mut owned) = runtimes.remove(&key) else {
                return Err(CapabilityStopFailure {
                    runtime,
                    error: deferred_operation(),
                });
            };
            drop(runtimes);
            let mut exited = matches!(owned.child.try_wait(), Ok(Some(_)));
            #[cfg(windows)]
            if !exited {
                // Job 调用失败后仍等待 child；只有未确认退出才返还 Runtime ownership。
                let _ = terminate_managed_job(&owned.job);
            }
            #[cfg(not(windows))]
            let stopped = if exited {
                true
            } else {
                match terminate_managed_process(&mut owned.child) {
                    Ok(()) => true,
                    Err(_) => match owned.child.try_wait() {
                        Ok(Some(_)) => {
                            exited = true;
                            true
                        }
                        Ok(None) | Err(_) => false,
                    },
                }
            };
            let deadline = Instant::now() + Duration::from_secs(10);
            while !exited && Instant::now() < deadline {
                match owned.child.try_wait() {
                    Ok(Some(_)) => {
                        exited = true;
                        break;
                    }
                    Ok(None) | Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                }
            }
            #[cfg(windows)]
            let stopped = exited;
            if stopped && exited {
                drop(owned.client);
                Ok(StopEvidence {
                    runtime_state: CapabilityRuntimeState::Stopped,
                })
            } else {
                self.runtimes.lock().await.insert(key, owned);
                Err(CapabilityStopFailure {
                    runtime,
                    error: deferred_operation(),
                })
            }
        })
    }
}

#[cfg(test)]
mod tests {
    include!("serena_capability_action_tests.rs");
    use super::*;
    use crate::{
        discovery::InstallationSource,
        workspace_capability::{
            CapabilityInstallationState, WorkspaceCapabilityManager, WorkspaceCapabilityRegistry,
        },
    };
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    /// 构造不依赖本机 Serena 的安装探测结果。
    fn installation(state: InstallationState, path: PathBuf, version: &str) -> SerenaInstallation {
        SerenaInstallation {
            state,
            source: InstallationSource::Path,
            path,
            version: version.into(),
            context: None,
            error: None,
        }
    }

    /// 构造只含 server-resolved canonical root 的测试 Lease。
    fn lease(root: PathBuf) -> WorkspaceLease {
        WorkspaceLease {
            workspace_id: "workspace-a".into(),
            canonical_root: root,
            generation: 7,
        }
    }

    /// 以指定 fixture 构造 Serena shell。
    fn provider(
        detected: SerenaInstallation,
        file_probe: impl Fn(&Path) -> Result<bool, CapabilityProviderError> + Send + Sync + 'static,
    ) -> SerenaCapabilityProvider {
        let runtime_directory = detected
            .path
            .parent()
            .unwrap_or_else(|| Path::new("fixture"))
            .join("runtime");
        SerenaCapabilityProvider::with_probes(
            Arc::new(move || detected.clone()),
            Arc::new(file_probe),
            runtime_directory,
        )
    }

    /// 可控的 Provider-private Client fixture，用于在不启动真实 Serena 服务的情况下验证 call ownership。
    struct FixtureRuntimeClient {
        response: String,
        calls: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
        entered: Arc<tokio::sync::Notify>,
        release: Option<Arc<tokio::sync::Barrier>>,
    }

    impl FixtureRuntimeClient {
        /// 创建立即完成的独立 Client fixture。
        fn ready(response: &str) -> Self {
            Self {
                response: response.into(),
                calls: std::sync::Mutex::new(Vec::new()),
                entered: Arc::new(tokio::sync::Notify::new()),
                release: None,
            }
        }

        /// 创建由 barrier 控制的 Client fixture，避免并发测试依赖 sleep。
        fn blocking(response: &str) -> Self {
            Self {
                response: response.into(),
                calls: std::sync::Mutex::new(Vec::new()),
                entered: Arc::new(tokio::sync::Notify::new()),
                release: Some(Arc::new(tokio::sync::Barrier::new(2))),
            }
        }
    }

    impl SerenaRuntimeClient for FixtureRuntimeClient {
        /// 记录已经按 Slot 选择的 upstream 调用；可选 barrier 专门暴露 map lock 跨 await 回归。
        fn call<'a>(
            &'a self,
            name: &'a str,
            arguments: serde_json::Value,
        ) -> CapabilityFuture<'a, Result<String, CapabilityProviderError>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((name.into(), arguments));
                self.entered.notify_one();
                if let Some(release) = &self.release {
                    release.wait().await;
                }
                Ok(self.response.clone())
            })
        }
    }

    /// 为 call fixture 构造受控 child；测试结束会显式回收，不向公开 Runtime handle 暴露其细节。
    fn fixture_runtime(client: Arc<dyn SerenaRuntimeClient>) -> SerenaRuntime {
        #[cfg(windows)]
        let child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let child = std::process::Command::new("sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .unwrap();
        #[cfg(windows)]
        let job = contain_process(&child).unwrap();
        SerenaRuntime {
            client,
            child,
            #[cfg(windows)]
            job,
        }
    }

    /// 安装一条仅供本模块测试的 Runtime，模拟已经由 Manager 验证并发布的 opaque handle。
    async fn insert_fixture_runtime(
        provider: &SerenaCapabilityProvider,
        lease: &WorkspaceLease,
        client: Arc<dyn SerenaRuntimeClient>,
    ) {
        provider
            .runtimes
            .lock()
            .await
            .insert(SerenaRuntimeKey::from_lease(lease), fixture_runtime(client));
    }

    /// 显式回收 fixture child，避免 focused call 测试留下后台进程。
    async fn remove_fixture_runtime(provider: &SerenaCapabilityProvider, lease: &WorkspaceLease) {
        let mut runtime = provider
            .runtimes
            .lock()
            .await
            .remove(&SerenaRuntimeKey::from_lease(lease))
            .unwrap();
        #[cfg(windows)]
        let _ = terminate_managed_job(&runtime.job);
        #[cfg(not(windows))]
        let _ = terminate_managed_process(&mut runtime.child);
        SerenaCapabilityProvider::cleanup_child(&mut runtime.child).await;
    }

    /// 用于验证 Serena 局部失败不会污染 Registry 的独立第二 Provider。
    struct SecondProvider {
        descriptor: WorkspaceCapabilityDescriptor,
    }

    impl SecondProvider {
        /// 构造与 Serena 无共享状态的固定 ready Provider。
        fn new() -> Self {
            Self {
                descriptor: WorkspaceCapabilityDescriptor {
                    provider_id: WorkspaceCapabilityProviderId::new("second"),
                    display_name: "Second".into(),
                    tool_names: vec![],
                    runtime_model: CapabilityRuntimeModel::InProcess,
                    readiness_probe: CapabilityReadinessProbe::Required,
                    preparation_policy:
                        crate::workspace_capability::CapabilityPreparationPolicy::None,
                    stage_descriptors: vec![],
                    action_descriptors: vec![],
                    runtime_policy: CapabilityRuntimePolicy {
                        max_instances: 1,
                        idle_timeout_ms: 1,
                        per_slot_concurrency: 1,
                    },
                },
            }
        }
    }

    impl WorkspaceCapabilityProvider for SecondProvider {
        /// 返回独立 fake Provider descriptor。
        fn descriptor(&self) -> &WorkspaceCapabilityDescriptor {
            &self.descriptor
        }

        /// 固定声明 fake Provider 已安装。
        fn probe_installation(
            &self,
        ) -> CapabilityFuture<'_, Result<CapabilityInstallation, CapabilityProviderError>> {
            Box::pin(async {
                Ok(CapabilityInstallation {
                    state: CapabilityInstallationState::Installed,
                    detected_version: Some("fixture".into()),
                })
            })
        }

        /// 固定返回 ready observation，验证 Serena probe error 不会影响它。
        fn observe_readiness(
            &self,
            _lease: WorkspaceLease,
        ) -> CapabilityFuture<
            '_,
            Result<crate::workspace_capability::CapabilityObservation, CapabilityProviderError>,
        > {
            let provider_id = self.descriptor.provider_id.clone();
            Box::pin(async move {
                Ok(crate::workspace_capability::CapabilityObservation {
                    provider_id,
                    installation: CapabilityInstallationState::Installed,
                    readiness: CapabilityReadinessState::Ready,
                    runtime_state: CapabilityRuntimeState::Stopped,
                    checked_at: 0,
                    stages: vec![],
                    actions: vec![],
                })
            })
        }

        /// 此测试不使用准备操作，固定 fail-closed。
        fn prepare<'a>(
            &'a self,
            _lease: WorkspaceLease,
            _action: CapabilityPrepareAction,
            _activity: &'a dyn CapabilityActivitySink,
        ) -> CapabilityFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>>
        {
            Box::pin(async { Err(deferred_operation()) })
        }

        /// 此测试不使用 Runtime，固定 fail-closed。
        fn start(
            &self,
            _lease: WorkspaceLease,
        ) -> CapabilityFuture<'_, Result<CapabilityRuntimeHandle, CapabilityProviderError>>
        {
            Box::pin(async { Err(deferred_operation()) })
        }

        /// 此测试不使用 Tool routing，固定 fail-closed。
        fn call<'a>(
            &'a self,
            _lease: &'a WorkspaceLease,
            _runtime: Option<&'a CapabilityRuntimeHandle>,
            _tool: WorkspaceToolCall,
        ) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>> {
            Box::pin(async { Err(deferred_operation()) })
        }

        /// 此测试不创建 Runtime，返还输入 handle 以满足 port ownership 契约。
        fn stop(
            &self,
            runtime: CapabilityRuntimeHandle,
        ) -> CapabilityFuture<'_, Result<StopEvidence, CapabilityStopFailure>> {
            Box::pin(async move {
                Err(CapabilityStopFailure {
                    runtime,
                    error: deferred_operation(),
                })
            })
        }
    }

    #[test]
    fn descriptor_projects_all_seven_workspace_scoped_source_tool_names() {
        let provider = provider(
            installation(
                InstallationState::Missing,
                PathBuf::from("missing-serena"),
                "",
            ),
            |_| Ok(false),
        );
        let descriptor = provider.descriptor();

        assert_eq!(descriptor.provider_id.as_str(), "serena");
        assert_eq!(descriptor.display_name, "Serena");
        assert_eq!(
            descriptor.tool_names,
            [
                "source_read_file",
                "source_list_dir",
                "source_find_file",
                "source_search_pattern",
                "source_symbols_overview",
                "source_find_symbol",
                "source_find_references"
            ]
        );
        assert_eq!(
            descriptor.runtime_model,
            CapabilityRuntimeModel::WorkspaceScopedProcess
        );
        assert_eq!(
            descriptor.readiness_probe,
            CapabilityReadinessProbe::Required
        );
        assert_eq!(
            descriptor.preparation_policy,
            crate::workspace_capability::CapabilityPreparationPolicy::AutoOnFirstToolCall
        );
        assert_eq!(descriptor.stage_descriptors.len(), 3);
        assert_eq!(descriptor.action_descriptors[0].action_id, "prepare");
        assert!(descriptor.action_descriptors[0].warm_runtime);
        assert_eq!(descriptor.action_descriptors[1].action_id, "build_index");
        assert!(!descriptor.action_descriptors[1].warm_runtime);
        assert_eq!(descriptor.runtime_policy.max_instances, 2);
        assert_eq!(descriptor.runtime_policy.per_slot_concurrency, 1);
        assert_eq!(descriptor.runtime_policy.idle_timeout_ms, 60_000);
    }

    #[tokio::test]
    /// 验证全部 Source name 复用 Serena Slot，并保留各自冻结的参数清理规则。
    async fn source_calls_map_all_frozen_tools_and_sanitize_upstream_arguments() {
        let provider = provider(
            installation(
                InstallationState::Missing,
                PathBuf::from("missing-serena"),
                "",
            ),
            |_| Ok(true),
        );
        let workspace_lease = lease(PathBuf::from("C:/workspace-a"));
        let client = Arc::new(FixtureRuntimeClient::ready("fixture result"));
        let client_port: Arc<dyn SerenaRuntimeClient> = client.clone();
        insert_fixture_runtime(&provider, &workspace_lease, client_port).await;
        let runtime = CapabilityRuntimeHandle::new(
            WorkspaceCapabilityProviderId::new("serena"),
            &workspace_lease,
        );

        for (tool_name, arguments, upstream_name, relative_path, max_answer_chars) in [
            (
                "source_symbols_overview",
                serde_json::json!({
                    "workspaceId":"workspace-a",
                    "relative_path":"src/lib.rs",
                    "depth":null,
                    "max_bytes":123
                }),
                "get_symbols_overview",
                "src/lib.rs",
                123,
            ),
            (
                "source_find_symbol",
                serde_json::json!({
                    "workspaceId":"workspace-a",
                    "relative_path":null,
                    "name_path_pattern":"Widget",
                    "depth":2,
                    "include_body":null,
                    "max_bytes":456
                }),
                "find_symbol",
                "",
                456,
            ),
            (
                "source_find_references",
                serde_json::json!({
                    "workspaceId":"workspace-a",
                    "relative_path":"src/lib.rs",
                    "name_path":"Widget",
                    "max_bytes":789
                }),
                "find_referencing_symbols",
                "src/lib.rs",
                789,
            ),
        ] {
            let result = provider
                .call(
                    &workspace_lease,
                    Some(&runtime),
                    WorkspaceToolCall {
                        tool_name: tool_name.into(),
                        arguments,
                    },
                )
                .await
                .unwrap();
            assert_eq!(result.result, serde_json::json!("fixture result"));
            let (received_name, received_arguments) = client.calls.lock().unwrap().pop().unwrap();
            assert_eq!(received_name, upstream_name);
            assert_eq!(received_arguments["relative_path"], relative_path);
            assert_eq!(received_arguments["max_answer_chars"], max_answer_chars);
            assert!(received_arguments.get("workspaceId").is_none());
            assert!(received_arguments.get("max_bytes").is_none());
            assert!(received_arguments.get("depth").is_none() || received_arguments["depth"] == 2);
            assert!(received_arguments.get("include_body").is_none());
        }

        for (tool_name, arguments, upstream_name, expects_max_answer_chars) in [
            (
                "source_read_file",
                serde_json::json!({"relative_path":"src/lib.rs","max_bytes":123}),
                "read_file",
                true,
            ),
            (
                "source_list_dir",
                serde_json::json!({"relative_path":"src","recursive":null}),
                "list_dir",
                true,
            ),
            (
                "source_find_file",
                serde_json::json!({"relative_path":null,"file_mask":"*.rs","max_bytes":456}),
                "find_file",
                false,
            ),
            (
                "source_search_pattern",
                serde_json::json!({"relative_path":"src","substring_pattern":"Workspace","max_bytes":789}),
                "search_for_pattern",
                true,
            ),
        ] {
            provider
                .call(
                    &workspace_lease,
                    Some(&runtime),
                    WorkspaceToolCall {
                        tool_name: tool_name.into(),
                        arguments,
                    },
                )
                .await
                .unwrap();
            let (received_name, received_arguments) = client.calls.lock().unwrap().pop().unwrap();
            assert_eq!(received_name, upstream_name);
            assert!(received_arguments.get("workspaceId").is_none());
            assert!(received_arguments.get("max_bytes").is_none());
            assert_eq!(
                received_arguments.get("max_answer_chars").is_some(),
                expects_max_answer_chars
            );
            if tool_name == "source_list_dir" {
                assert_eq!(received_arguments["recursive"], false);
            }
        }
        assert!(
            provider
                .call(
                    &workspace_lease,
                    Some(&runtime),
                    WorkspaceToolCall {
                        tool_name: "source_find_symbol".into(),
                        arguments: serde_json::json!({
                            "name_path_pattern":"Widget",
                            "root":"C:/caller-supplied-root"
                        }),
                    },
                )
                .await
                .is_err()
        );
        assert!(
            provider
                .call(
                    &workspace_lease,
                    Some(&runtime),
                    WorkspaceToolCall {
                        tool_name: "source_find_symbol".into(),
                        arguments: serde_json::json!({
                            "name_path_pattern":"Widget",
                            "relative_path":"C:/caller-supplied-root"
                        }),
                    },
                )
                .await
                .is_err()
        );
        assert!(client.calls.lock().unwrap().is_empty());
        remove_fixture_runtime(&provider, &workspace_lease).await;
    }

    #[tokio::test]
    /// 验证不同 Slot 在 Client await 期间不会持有 Provider 全局 Runtime map 锁，也不会串线。
    async fn semantic_calls_for_distinct_slots_use_their_private_clients_concurrently() {
        let provider = Arc::new(provider(
            installation(
                InstallationState::Missing,
                PathBuf::from("missing-serena"),
                "",
            ),
            |_| Ok(true),
        ));
        let workspace_a = lease(PathBuf::from("C:/workspace-a"));
        let mut workspace_b = lease(PathBuf::from("C:/workspace-b"));
        workspace_b.workspace_id = "workspace-b".into();
        let client_a = Arc::new(FixtureRuntimeClient::blocking("A"));
        let client_b = Arc::new(FixtureRuntimeClient::blocking("B"));
        let client_a_port: Arc<dyn SerenaRuntimeClient> = client_a.clone();
        let client_b_port: Arc<dyn SerenaRuntimeClient> = client_b.clone();
        insert_fixture_runtime(&provider, &workspace_a, client_a_port).await;
        insert_fixture_runtime(&provider, &workspace_b, client_b_port).await;
        let runtime_a = CapabilityRuntimeHandle::new(
            WorkspaceCapabilityProviderId::new("serena"),
            &workspace_a,
        );
        let runtime_b = CapabilityRuntimeHandle::new(
            WorkspaceCapabilityProviderId::new("serena"),
            &workspace_b,
        );

        let first_entered = client_a.entered.notified();
        let provider_a = Arc::clone(&provider);
        let first_lease = workspace_a.clone();
        let first = tokio::spawn(async move {
            provider_a
                .call(
                    &first_lease,
                    Some(&runtime_a),
                    WorkspaceToolCall {
                        tool_name: "source_find_symbol".into(),
                        arguments: serde_json::json!({"name_path_pattern":"A"}),
                    },
                )
                .await
        });
        first_entered.await;

        let second_entered = client_b.entered.notified();
        let provider_b = Arc::clone(&provider);
        let second_lease = workspace_b.clone();
        let second = tokio::spawn(async move {
            provider_b
                .call(
                    &second_lease,
                    Some(&runtime_b),
                    WorkspaceToolCall {
                        tool_name: "source_find_symbol".into(),
                        arguments: serde_json::json!({"name_path_pattern":"B"}),
                    },
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), second_entered)
            .await
            .expect(
                "second Slot must enter its private Client call without waiting for first Slot",
            );

        client_a.release.as_ref().unwrap().wait().await;
        client_b.release.as_ref().unwrap().wait().await;
        assert_eq!(first.await.unwrap().unwrap().result, serde_json::json!("A"));
        assert_eq!(
            second.await.unwrap().unwrap().result,
            serde_json::json!("B")
        );
        assert_eq!(
            client_a.calls.lock().unwrap().as_slice(),
            [(
                "find_symbol".into(),
                serde_json::json!({
                    "name_path_pattern":"A",
                    "relative_path":"",
                    "max_answer_chars":65536
                })
            )]
        );
        assert_eq!(
            client_b.calls.lock().unwrap().as_slice(),
            [(
                "find_symbol".into(),
                serde_json::json!({
                    "name_path_pattern":"B",
                    "relative_path":"",
                    "max_answer_chars":65536
                })
            )]
        );
        remove_fixture_runtime(&provider, &workspace_a).await;
        remove_fixture_runtime(&provider, &workspace_b).await;
    }

    #[test]
    fn slot_home_is_deterministic_safe_and_isolated_by_workspace_and_generation() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("serena.exe");
        std::fs::write(&binary, "fixture").unwrap();
        let provider = provider(
            installation(InstallationState::Standard, binary, "Serena 1.7.0"),
            |_| Ok(true),
        );
        let first = lease(PathBuf::from("C:/workspace-a"));
        let mut second = first.clone();
        second.workspace_id = "legacy/unsafe:workspace".into();
        let mut next_generation = first.clone();
        next_generation.generation = 8;

        let first_home = provider.slot_home(&first);
        assert_eq!(first_home, provider.slot_home(&first));
        assert_ne!(first_home, provider.slot_home(&second));
        assert_ne!(first_home, provider.slot_home(&next_generation));
        assert!(!first_home.to_string_lossy().contains("workspace-a"));
        assert!(first_home.starts_with(directory.path().join("runtime")));
    }

    #[test]
    fn slot_config_uses_fixed_safe_fields_and_never_imports_shared_projects() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("managed-slot");
        let context = home.join("workspace-context.yml");
        config::prepare_workspace_serena_home(&home, &context).unwrap();

        let global: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(home.join("serena_config.yml")).unwrap())
                .unwrap();
        let context: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(context).unwrap()).unwrap();
        assert_eq!(
            global["trusted_project_path_patterns"],
            serde_json::json!([])
        );
        assert_eq!(global["web_dashboard"], false);
        assert_eq!(global["gui_log_window"], false);
        assert_eq!(
            global["project_serena_folder_location"],
            "$projectDir/.serena"
        );
        assert_eq!(global["projects"], serde_json::json!([]));
        assert_eq!(context["single_project"], false);
        assert!(
            context["fixed_tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool == "activate_project")
        );
        assert!(
            context["fixed_tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool == "get_current_config")
        );
    }

    #[tokio::test]
    async fn missing_binary_is_not_installed_without_workspace_file_probe() {
        let file_probes = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&file_probes);
        let provider = provider(
            installation(
                InstallationState::Missing,
                PathBuf::from("missing-serena"),
                "",
            ),
            move |_| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(false)
            },
        );

        assert_eq!(
            provider.probe_installation().await.unwrap().state,
            CapabilityInstallationState::NotInstalled
        );
        let observation = provider
            .observe_readiness(lease(PathBuf::from("C:/lease-root")))
            .await
            .unwrap();
        assert_eq!(
            observation.installation,
            CapabilityInstallationState::NotInstalled
        );
        assert_eq!(observation.readiness, CapabilityReadinessState::Unknown);
        assert_eq!(file_probes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn incompatible_version_is_check_failed_with_no_detected_version() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("serena.exe");
        std::fs::write(&binary, "fixture").unwrap();
        let provider = provider(
            installation(InstallationState::Invalid, binary, "Serena 1.6.9"),
            |_| Ok(false),
        );

        let installation = provider.probe_installation().await.unwrap();
        assert_eq!(installation.state, CapabilityInstallationState::CheckFailed);
        assert_eq!(installation.detected_version, None);
    }

    #[tokio::test]
    async fn ready_configuration_uses_only_lease_root_and_keeps_optional_stages_unknown() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let binary = directory.path().join("serena.exe");
        std::fs::write(&binary, "fixture").unwrap();
        let expected_root = root.clone();
        let provider = provider(
            installation(InstallationState::Standard, binary, "Serena 1.7.0"),
            move |actual_root| {
                assert_eq!(actual_root, expected_root);
                Ok(true)
            },
        );

        let observation = provider
            .observe_readiness(lease(root.clone()))
            .await
            .unwrap();
        assert_eq!(
            provider
                .probe_installation()
                .await
                .unwrap()
                .detected_version
                .as_deref(),
            Some("Serena 1.7.0")
        );
        assert_eq!(
            observation.installation,
            CapabilityInstallationState::Installed
        );
        assert_eq!(observation.readiness, CapabilityReadinessState::Ready);
        assert_eq!(observation.runtime_state, CapabilityRuntimeState::Stopped);
        assert_eq!(observation.stages[0].state, CapabilityStageState::Ready);
        assert_eq!(observation.stages[1].state, CapabilityStageState::Unknown);
        assert_eq!(observation.stages[2].state, CapabilityStageState::Unknown);
        assert!(!root.join(".serena").exists());
    }

    #[tokio::test]
    async fn absent_configuration_enters_official_auto_prepare_start_path() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let binary = directory.path().join("serena.exe");
        std::fs::write(&binary, "fixture").unwrap();
        let provider = provider(
            installation(InstallationState::Standard, binary, "Serena 1.7.0"),
            |_| Ok(false),
        );

        let observation = provider
            .observe_readiness(lease(root.clone()))
            .await
            .unwrap();
        assert_eq!(observation.readiness, CapabilityReadinessState::NotPrepared);
        assert_eq!(observation.runtime_state, CapabilityRuntimeState::Stopped);
        assert_eq!(observation.stages[0].state, CapabilityStageState::Absent);
        assert_eq!(
            observation.stages[0].requirement,
            CapabilityStageRequirement::AutoPreparable
        );
        assert_eq!(observation.stages[1].state, CapabilityStageState::Unknown);
        assert_eq!(observation.stages[2].state, CapabilityStageState::Unknown);
        // readiness probe 保持只读；真正的首个 acquire 由下方 official smoke 覆盖。
        assert!(!root.join(".serena").exists());
    }

    #[tokio::test]
    async fn existing_configuration_is_not_mutated_by_readiness_probe() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        let project_configuration = root.join(".serena").join("project.yml");
        std::fs::create_dir_all(project_configuration.parent().unwrap()).unwrap();
        std::fs::write(
            &project_configuration,
            "preserve-existing-project-configuration\n",
        )
        .unwrap();
        let binary = directory.path().join("serena.exe");
        std::fs::write(&binary, "fixture").unwrap();
        let provider = provider(
            installation(InstallationState::Standard, binary, "Serena 1.7.0"),
            project_configuration_exists,
        );

        let observation = provider.observe_readiness(lease(root)).await.unwrap();
        assert_eq!(observation.stages[0].state, CapabilityStageState::Ready);
        assert_eq!(
            std::fs::read_to_string(project_configuration).unwrap(),
            "preserve-existing-project-configuration\n"
        );
    }

    #[tokio::test]
    async fn pre_probe_error_fails_closed_before_creating_slot_home_or_child() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("serena.exe");
        std::fs::write(&binary, "fixture").unwrap();
        let provider = provider(
            installation(InstallationState::Standard, binary, "Serena 1.7.0"),
            |_| {
                Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::OperationFailed,
                })
            },
        );

        assert_eq!(
            provider
                .start(lease(directory.path().join("workspace")))
                .await,
            Err(CapabilityProviderError {
                code: CapabilityProviderErrorCode::OperationFailed,
            })
        );
        assert!(!directory.path().join("runtime").exists());
    }

    #[tokio::test]
    async fn project_configuration_directory_is_absent_without_creating_other_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        let project_configuration = root.join(".serena").join("project.yml");
        std::fs::create_dir_all(&project_configuration).unwrap();
        let binary = directory.path().join("serena.exe");
        std::fs::write(&binary, "fixture").unwrap();
        // 使用生产 metadata probe，而非 mock，固定验证目录不能冒充 Project Configuration 文件。
        let provider = provider(
            installation(InstallationState::Standard, binary, "Serena 1.7.0"),
            project_configuration_exists,
        );

        let observation = provider
            .observe_readiness(lease(root.clone()))
            .await
            .unwrap();

        assert_eq!(observation.readiness, CapabilityReadinessState::NotPrepared);
        assert_eq!(observation.stages[0].state, CapabilityStageState::Absent);
        assert_eq!(observation.stages[1].state, CapabilityStageState::Unknown);
        assert_eq!(observation.stages[2].state, CapabilityStageState::Unknown);
        assert!(project_configuration.is_dir());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(root.join(".serena")).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn file_probe_failure_isolated_from_second_registered_provider() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("serena.exe");
        std::fs::write(&binary, "fixture").unwrap();
        let serena: Arc<dyn WorkspaceCapabilityProvider> = Arc::new(provider(
            installation(InstallationState::Standard, binary, "Serena 1.7.0"),
            |_| {
                Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::OperationFailed,
                })
            },
        ));
        let second: Arc<dyn WorkspaceCapabilityProvider> = Arc::new(SecondProvider::new());
        let registry =
            WorkspaceCapabilityRegistry::new([Arc::clone(&serena), Arc::clone(&second)]).unwrap();

        assert_eq!(registry.providers().len(), 2);
        assert_eq!(
            serena
                .observe_readiness(lease(directory.path().join("workspace")))
                .await,
            Err(CapabilityProviderError {
                code: CapabilityProviderErrorCode::OperationFailed
            })
        );
        assert_eq!(
            second
                .observe_readiness(lease(directory.path().join("other-workspace")))
                .await
                .unwrap()
                .readiness,
            CapabilityReadinessState::Ready
        );
    }

    #[tokio::test]
    #[ignore = "requires SERENA_TEST_EXE pointing at the official 1.7.0 test installation"]
    async fn official_serena_first_acquire_auto_creates_project_configuration() {
        let executable =
            PathBuf::from(std::env::var_os("SERENA_TEST_EXE").expect("set SERENA_TEST_EXE"));
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("example.py"), "def marker():\n    return 1\n").unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let project_configuration = root.join(".serena").join("project.yml");
        assert!(!project_configuration.exists());
        let installation = installation(InstallationState::Standard, executable, "Serena 1.7.0");
        let provider = Arc::new(SerenaCapabilityProvider::with_probes(
            Arc::new(move || installation.clone()),
            Arc::new(project_configuration_exists),
            directory.path().join("runtime"),
        ));
        let provider_port: Arc<dyn WorkspaceCapabilityProvider> = provider.clone();
        let manager = WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new([provider_port]).unwrap(),
        ));
        let lease = WorkspaceLease {
            workspace_id: "workspace".into(),
            canonical_root: root,
            generation: 1,
        };
        let runtime = manager
            .acquire_runtime("serena", lease.clone())
            .await
            .unwrap();
        assert!(
            provider
                .slot_home(&lease)
                .join("serena_config.yml")
                .is_file()
        );
        // 本测试只调用 Runtime acquire，不会调用 index/onboarding 或 Provider prepare。
        assert!(project_configuration.is_file());
        drop(runtime);
        manager.shutdown_runtimes().await.unwrap();
        assert!(project_configuration.is_file());
        let existing_project_configuration = std::fs::read(&project_configuration).unwrap();

        // 以新的 Manager 模拟后续 acquire；既有默认 Project Configuration 不得被重建或改写。
        let provider_port: Arc<dyn WorkspaceCapabilityProvider> = provider.clone();
        let second_manager = WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new([provider_port]).unwrap(),
        ));
        let runtime = second_manager
            .acquire_runtime("serena", lease)
            .await
            .unwrap();
        drop(runtime);
        second_manager.shutdown_runtimes().await.unwrap();
        assert_eq!(
            std::fs::read(project_configuration).unwrap(),
            existing_project_configuration
        );
    }

    #[tokio::test]
    #[ignore = "requires SERENA_TEST_EXE pointing at the official 1.7.0 test installation"]
    async fn official_serena_postcondition_failure_keeps_project_file_without_publishing_runtime() {
        let executable =
            PathBuf::from(std::env::var_os("SERENA_TEST_EXE").expect("set SERENA_TEST_EXE"));
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("example.py"), "def marker():\n    return 1\n").unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let project_configuration = root.join(".serena").join("project.yml");
        let probes = Arc::new(AtomicUsize::new(0));
        let probe_count = Arc::clone(&probes);
        let installation = installation(InstallationState::Standard, executable, "Serena 1.7.0");
        let provider = Arc::new(SerenaCapabilityProvider::with_probes(
            Arc::new(move || installation.clone()),
            Arc::new(move |_| {
                // 第二次 probe 故意拒绝，以验证已创建的文件不会因 postcondition 失败被回滚。
                probe_count.fetch_add(1, Ordering::SeqCst);
                Ok(false)
            }),
            directory.path().join("runtime"),
        ));
        let provider_port: Arc<dyn WorkspaceCapabilityProvider> = provider.clone();
        let manager = WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new([provider_port]).unwrap(),
        ));
        let lease = WorkspaceLease {
            workspace_id: "workspace".into(),
            canonical_root: root,
            generation: 1,
        };

        assert!(manager.acquire_runtime("serena", lease).await.is_err());
        assert_eq!(probes.load(Ordering::SeqCst), 2);
        assert!(project_configuration.is_file());
        assert!(provider.runtimes.lock().await.is_empty());
        manager.shutdown_runtimes().await.unwrap();
        assert!(project_configuration.is_file());
    }
}

pub use crate::discovery::SerenaInstallation;
#[cfg(test)]
pub(crate) mod remote_fixture;
use crate::discovery::{self, GitInstallation, InstallationState};
use crate::{
    agent::{
        product::AgentProductService,
        store::{
            StateStore,
            transactions::{
                CreateOutcome,
                product::{WorkExecutionContext, WorkspaceSnapshot},
            },
        },
    },
    codegraph_capability::CodeGraphCapabilityProvider,
    config::{self, AppPaths, ManagerConfig, Workspace},
    logs,
    mcp::capability_adapters::{GitCapabilityProvider, SourceCapabilityProvider},
    serena_capability::SerenaCapabilityProvider,
    workspace_capability::{
        WorkspaceCapabilityErrorCode, WorkspaceCapabilityManager, WorkspaceCapabilityRegistry,
    },
    workspace_resolver::{WorkspaceLease, WorkspaceResolver},
};
use serde::Serialize;
use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    io::{BufRead, BufReader, Read},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const DEFAULT_DASHBOARD_URL: &str = "http://127.0.0.1:24282/dashboard/index.html";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Error,
}

struct ManagedProcess {
    child: Child,
    #[cfg(windows)]
    _job: std::os::windows::io::OwnedHandle,
    port: u16,
    dashboard_enabled: bool,
    installation: SerenaInstallation,
}

struct Runtime {
    config: ManagerConfig,
    installation: Option<SerenaInstallation>,
    git: GitInstallation,
    codegraph_version: Option<String>,
    process: Option<ManagedProcess>,
    status: ServerStatus,
    last_error: Option<String>,
}

#[cfg(test)]
type RemoteSaveHook = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;
#[cfg(test)]
type WorkspaceStartHook = Arc<dyn Fn() + Send + Sync>;
#[cfg(test)]
type WorkspaceRemoveHook = Arc<dyn Fn() + Send + Sync>;

/// Remove 在既有 operation mutex 内汇总的固定三类 Workspace owner。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkspaceRemoveOwners {
    agent_claim_present: bool,
    write_guard_count: u32,
    future_runtime_slot_ref_count: u32,
}

impl WorkspaceRemoveOwners {
    /// 任一明确 owner 存在即拒绝 Remove，绝不执行 best-effort 删除。
    fn is_busy(self) -> bool {
        self.agent_claim_present
            || self.write_guard_count != 0
            || self.future_runtime_slot_ref_count != 0
    }
}

/// 仅测试模拟后续 RuntimeSlot owner；不会编译进生产状态或形成动态注册机制。
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WorkspaceRemoveTestOwnerCounts {
    pub(crate) future_runtime_slot_ref_count: u32,
}

/// Guard 的内部身份必须与当次解析到的 Workspace Lease 完全一致。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WorkspaceWriteGuardIdentity {
    workspace_id: String,
    generation: u64,
    canonical_root: PathBuf,
}

impl From<&WorkspaceLease> for WorkspaceWriteGuardIdentity {
    /// 仅复制同一 operation 临界区解析出的 Lease，不接受调用方提供的 root。
    fn from(lease: &WorkspaceLease) -> Self {
        Self {
            workspace_id: lease.workspace_id.clone(),
            generation: lease.generation,
            canonical_root: lease.canonical_root.clone(),
        }
    }
}

/// Supervisor 进程内维护的活跃 Source Write 生命周期计数，不是文件写入互斥锁。
#[derive(Default)]
struct WorkspaceWriteGuardState {
    counts: HashMap<WorkspaceWriteGuardIdentity, u32>,
}

/// 活跃 Source Write 的 RAII owner；只能由 Supervisor 解析当前 Lease 后创建。
pub(crate) struct WorkspaceWriteGuard {
    identity: WorkspaceWriteGuardIdentity,
    state: Arc<Mutex<WorkspaceWriteGuardState>>,
}

impl Drop for WorkspaceWriteGuard {
    /// 生命周期结束时递减 refcount，并在最后一个 Guard 释放后删除状态条目。
    fn drop(&mut self) {
        let mut state = self
            .state
            .lock()
            .expect("workspace write guard mutex poisoned");
        let Some(count) = state.counts.get_mut(&self.identity) else {
            debug_assert!(
                false,
                "workspace write guard must retain its refcount entry"
            );
            return;
        };
        if *count == 1 {
            state.counts.remove(&self.identity);
        } else {
            *count -= 1;
        }
    }
}

/// 已通过普通异步 preflight 的 Start 持久化输入；Workspace 快照仅在 operation mutex 内解析。
pub(crate) struct WorkspaceStartCreation {
    pub(crate) execution_id: String,
    pub(crate) agent_id: String,
    pub(crate) request_key: String,
    pub(crate) prompt: String,
    pub(crate) workspace_id: String,
    pub(crate) work: Option<WorkExecutionContext>,
    pub(crate) now: i64,
}

pub struct SupervisorState {
    runtime: Arc<Mutex<Runtime>>,
    operation: Mutex<()>,
    capability_manager: Arc<WorkspaceCapabilityManager>,
    workspace_write_guards: Arc<Mutex<WorkspaceWriteGuardState>>,
    target_commit_coordinator: crate::mcp::source_write_commit::TargetCommitCoordinator,
    dashboard_url: Arc<Mutex<String>>,
    pub paths: AppPaths,
    #[cfg(test)]
    pub(crate) remote_save_hook: Mutex<Option<RemoteSaveHook>>,
    #[cfg(test)]
    pub(crate) workspace_start_hook: Mutex<Option<WorkspaceStartHook>>,
    #[cfg(test)]
    pub(crate) workspace_remove_hook: Mutex<Option<WorkspaceRemoveHook>>,
    #[cfg(test)]
    pub(crate) workspace_remove_test_owners: Mutex<WorkspaceRemoveTestOwnerCounts>,
}

#[derive(Debug, Clone)]
pub struct SupervisorSnapshot {
    pub git: GitInstallation,
    pub codegraph_version: Option<String>,
    pub config: ManagerConfig,
    pub installation: Option<SerenaInstallation>,
    pub active_installation: Option<SerenaInstallation>,
    pub server_status: ServerStatus,
    pub managed_process_present: bool,
    pub active_port: u16,
    pub process_id: Option<u32>,
    pub active_dashboard_enabled: bool,
    pub dashboard_url: String,
    pub last_error: Option<String>,
}

#[derive(Debug)]
pub struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl SupervisorState {
    pub fn new(paths: AppPaths) -> Result<Self, String> {
        logs::ensure_directory(&paths.log_directory)?;
        let config = config::load(&paths.config_file)?;
        // Provider 只读取最新配置快照；Registry 构造本身不执行 Serena CLI 或 Runtime 生命周期。
        let runtime = Arc::new(Mutex::new(Runtime {
            config,
            installation: None,
            git: GitInstallation::default(),
            codegraph_version: None,
            process: None,
            status: ServerStatus::Stopped,
            last_error: None,
        }));
        let config_runtime = Arc::clone(&runtime);
        let serena_provider: Arc<dyn crate::workspace_capability::WorkspaceCapabilityProvider> =
            Arc::new(SerenaCapabilityProvider::new(
                Arc::new(move || {
                    config_runtime
                        .lock()
                        .expect("supervisor runtime mutex poisoned")
                        .config
                        .clone()
                }),
                paths.clone(),
            ));
        let source_provider: Arc<dyn crate::workspace_capability::WorkspaceCapabilityProvider> =
            Arc::new(SourceCapabilityProvider::new());
        let git_provider: Arc<dyn crate::workspace_capability::WorkspaceCapabilityProvider> =
            Arc::new(GitCapabilityProvider::new());
        // CodeGraph 在 Provider 内部使用 RuntimeSlot；Remote query tool 仍由 P2D-009 Gate 禁用。
        let codegraph_provider: Arc<dyn crate::workspace_capability::WorkspaceCapabilityProvider> =
            Arc::new(CodeGraphCapabilityProvider::new());
        Ok(Self {
            runtime,
            operation: Mutex::new(()),
            capability_manager: Arc::new(WorkspaceCapabilityManager::new(Arc::new(
                WorkspaceCapabilityRegistry::new([
                    serena_provider,
                    source_provider,
                    git_provider,
                    codegraph_provider,
                ])
                .map_err(|_| "workspace capability registry initialization failed".to_owned())?,
            ))),
            workspace_write_guards: Arc::new(Mutex::new(WorkspaceWriteGuardState::default())),
            target_commit_coordinator:
                crate::mcp::source_write_commit::TargetCommitCoordinator::new(),
            dashboard_url: Arc::new(Mutex::new(DEFAULT_DASHBOARD_URL.to_string())),
            paths,
            #[cfg(test)]
            remote_save_hook: Mutex::new(None),
            #[cfg(test)]
            workspace_start_hook: Mutex::new(None),
            #[cfg(test)]
            workspace_remove_hook: Mutex::new(None),
            #[cfg(test)]
            workspace_remove_test_owners: Mutex::new(WorkspaceRemoveTestOwnerCounts::default()),
        })
    }

    /// 仅供跨层生命周期测试注入 fake Runtime Manager，不形成生产动态 Provider 注册机制。
    #[cfg(test)]
    pub(crate) fn replace_workspace_capability_manager_for_test(
        &mut self,
        manager: Arc<WorkspaceCapabilityManager>,
    ) {
        self.capability_manager = manager;
    }

    /// 返回已在 Supervisor 构造期固定的 Capability Manager，不暴露 Provider 私有 Runtime。
    pub(crate) fn workspace_capability_manager(&self) -> Arc<WorkspaceCapabilityManager> {
        Arc::clone(&self.capability_manager)
    }

    /// 返回本 Supervisor 独占的 per-target commit coordinator；不同 fixture 绝不共享锁表。
    pub(crate) fn target_commit_coordinator(
        &self,
    ) -> crate::mcp::source_write_commit::TargetCommitCoordinator {
        self.target_commit_coordinator.clone()
    }

    /// 验证 Guard 仍由本 Supervisor 为同一 Lease 持有，避免跨 Supervisor 或过期 Guard 混用。
    pub(crate) fn workspace_write_guard_matches(
        &self,
        guard: &WorkspaceWriteGuard,
        lease: &WorkspaceLease,
    ) -> bool {
        guard.identity == WorkspaceWriteGuardIdentity::from(lease)
            && Arc::ptr_eq(&guard.state, &self.workspace_write_guards)
            && guard
                .state
                .lock()
                .expect("workspace write guard mutex poisoned")
                .counts
                .get(&guard.identity)
                .copied()
                .unwrap_or(0)
                != 0
    }

    /// 在同一 operation 临界区内解析当前 Lease 并登记 Source Write 生命周期。
    pub(crate) fn resolve_workspace_write_guard(
        &self,
        workspace_id: &str,
    ) -> Result<(WorkspaceLease, WorkspaceWriteGuard), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        let lease = WorkspaceResolver::new(self).resolve(workspace_id)?;
        if self
            .capability_manager
            .workspace_remove_admission_active(&lease)
        {
            return Err(crate::workspace_registry::WORKSPACE_IN_USE.into());
        }
        let identity = WorkspaceWriteGuardIdentity::from(&lease);
        let mut state = self
            .workspace_write_guards
            .lock()
            .expect("workspace write guard mutex poisoned");
        let count = state.counts.entry(identity.clone()).or_insert(0);
        *count = count
            .checked_add(1)
            .expect("workspace write guard refcount overflow");
        Ok((
            lease,
            WorkspaceWriteGuard {
                identity,
                state: Arc::clone(&self.workspace_write_guards),
            },
        ))
    }

    /// 仅供本模块回归测试观察私有 refcount，不形成生产 introspection API。
    #[cfg(test)]
    fn workspace_write_guard_count_for_test(&self, lease: &WorkspaceLease) -> u32 {
        self.workspace_write_guards
            .lock()
            .expect("workspace write guard mutex poisoned")
            .counts
            .get(&WorkspaceWriteGuardIdentity::from(lease))
            .copied()
            .unwrap_or(0)
    }

    pub fn snapshot(&self) -> SupervisorSnapshot {
        self.refresh_process_status();
        let runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        let active_port = runtime
            .process
            .as_ref()
            .map(|process| process.port)
            .unwrap_or(runtime.config.port);
        let managed_process_present = runtime.process.is_some();
        let active_dashboard_enabled = runtime
            .process
            .as_ref()
            .map(|process| process.dashboard_enabled)
            .unwrap_or(runtime.config.dashboard_enabled);
        let active_installation = runtime
            .process
            .as_ref()
            .map(|process| process.installation.clone())
            .or_else(|| runtime.installation.clone());
        SupervisorSnapshot {
            process_id: runtime.process.as_ref().map(|p| p.child.id()),
            git: runtime.git.clone(),
            codegraph_version: runtime.codegraph_version.clone(),
            config: runtime.config.clone(),
            installation: runtime.installation.clone(),
            active_installation,
            server_status: runtime.status,
            managed_process_present,
            active_port,
            active_dashboard_enabled,
            dashboard_url: self
                .dashboard_url
                .lock()
                .expect("dashboard URL mutex poisoned")
                .clone(),
            last_error: runtime.last_error.clone(),
        }
    }

    pub fn detect_serena(&self) -> Option<SerenaInstallation> {
        let config = self
            .runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .config
            .clone();
        let installation = Some(discovery::detect(&config, &self.paths));
        self.detect_git();
        let version = discovery::detect_codegraph_version();
        self.runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .codegraph_version = version;
        self.commit_detection(&config, installation)
    }

    fn commit_detection(
        &self,
        detected_config: &ManagerConfig,
        installation: Option<SerenaInstallation>,
    ) -> Option<SerenaInstallation> {
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        if runtime.config != *detected_config {
            return runtime.installation.clone();
        }
        runtime.installation = installation.clone();
        if installation
            .as_ref()
            .is_some_and(|value| value.state == InstallationState::Standard)
            && runtime.status == ServerStatus::Error
            && runtime.process.is_none()
        {
            runtime.status = ServerStatus::Stopped;
            runtime.last_error = None;
        }
        installation
    }

    pub fn detect_git(&self) -> GitInstallation {
        let git = discovery::detect_git();
        self.runtime.lock().expect("supervisor mutex poisoned").git = git.clone();
        git
    }

    pub fn install(&self) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        if self.snapshot().managed_process_present {
            return Err("请先停止 Serena，再安装或修复 Managed 官方 Serena。".into());
        }
        let git = self.detect_git();
        if !git.available {
            return Err(git.error.unwrap_or_else(|| "Git 不可用。".into()));
        }
        let result = crate::installer::install_serena(&self.paths);
        self.detect_serena();
        result
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "仅供 Source Write workspace-generation drift 回归替换 workspace registry。"
        )
    )]
    pub fn replace_workspaces(
        &self,
        workspaces: Vec<crate::config::Workspace>,
    ) -> Result<(), String> {
        let _operation = self.operation.lock().expect("supervisor mutex poisoned");
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        let mut next = runtime.config.clone();
        next.workspaces = workspaces;
        next.validate()?;
        config::save(&self.paths.config_file, &next)?;
        runtime.config = next;
        Ok(())
    }

    pub(crate) fn workspace_registry_config(&self) -> ManagerConfig {
        self.runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .config
            .clone()
    }

    pub(crate) fn desktop_selected_workspace(&self) -> Option<Workspace> {
        let config = self.workspace_registry_config();
        config.desktop_selected_workspace_id.and_then(|id| {
            config
                .workspaces
                .into_iter()
                .find(|workspace| workspace.id == id)
        })
    }

    pub(crate) fn select_desktop_workspace(&self, id: &str) -> Result<Workspace, String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        let currently_selected = self.desktop_selected_workspace();
        let current = self
            .runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .config
            .clone();
        let workspace = current
            .workspaces
            .iter()
            .find(|workspace| workspace.id == id)
            .cloned()
            .ok_or_else(|| String::from(crate::workspace_registry::WORKSPACE_NOT_FOUND))?;
        if currently_selected
            .as_ref()
            .is_some_and(|selected| selected.id == id)
        {
            return Ok(workspace);
        }
        let mut next = current;
        next.desktop_selected_workspace_id = Some(id.to_owned());
        config::save(&self.paths.config_file, &next)?;
        self.runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .config = next;
        Ok(workspace)
    }

    pub(crate) fn mutate_workspace_registry(
        &self,
        mutation: impl FnOnce(&mut Vec<Workspace>) -> Result<(), String>,
    ) -> Result<bool, String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        self.mutate_workspace_registry_locked(mutation)
    }

    /// 调用方已持有 operation mutex 时复用 Registry 的原子配置提交逻辑。
    fn mutate_workspace_registry_locked(
        &self,
        mutation: impl FnOnce(&mut Vec<Workspace>) -> Result<(), String>,
    ) -> Result<bool, String> {
        let current = self
            .runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .config
            .clone();
        let mut next = current.clone();
        mutation(&mut next.workspaces)?;
        if next
            .desktop_selected_workspace_id
            .as_ref()
            .is_some_and(|id| !next.workspaces.iter().any(|workspace| workspace.id == *id))
        {
            next.desktop_selected_workspace_id = None;
        }
        if next.workspaces == current.workspaces {
            return Ok(false);
        }
        next.workspace_registry_revision = next
            .workspace_registry_revision
            .checked_add(1)
            .ok_or("internal workspace registry revision overflow")?;
        next.validate()?;
        config::save(&self.paths.config_file, &next)?;
        self.runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .config = next;
        Ok(true)
    }

    /// 只聚合冻结的 owner 类型；未来两类在生产固定为零，当前不查询进程、表或 Provider。
    fn workspace_remove_owners(
        &self,
        product: &AgentProductService,
        workspace: &Workspace,
        lease: &WorkspaceLease,
    ) -> Result<WorkspaceRemoveOwners, String> {
        let agent_claim_present =
            product.workspace_claim_exists_blocking(&workspace.root.to_string_lossy())?;
        let future_runtime_slot_ref_count = {
            #[cfg(test)]
            {
                let fixture = *self.workspace_remove_test_owners.lock().unwrap();
                fixture.future_runtime_slot_ref_count
            }
            #[cfg(not(test))]
            {
                0
            }
        };
        Ok(WorkspaceRemoveOwners {
            agent_claim_present,
            write_guard_count: self.workspace_write_guard_count(lease),
            future_runtime_slot_ref_count,
        })
    }

    /// 读取当前 Lease 对应的真实 Guard 数量；调用方已持有 operation mutex。
    fn workspace_write_guard_count(&self, lease: &WorkspaceLease) -> u32 {
        self.workspace_write_guards
            .lock()
            .expect("workspace write guard mutex poisoned")
            .counts
            .get(&WorkspaceWriteGuardIdentity::from(lease))
            .copied()
            .unwrap_or(0)
    }

    /// Remove 先在 capability Manager 中关闭 admission，stop 完成前绝不进入 Registry 删除。
    pub(crate) async fn remove_workspace_coordinated(
        &self,
        product: &AgentProductService,
        id: &str,
    ) -> Result<Workspace, String> {
        let (lease, removal) = {
            let _operation = self.operation.lock().expect("operation mutex poisoned");
            let lease = WorkspaceResolver::new(self).resolve(id)?;
            let removal = self
                .capability_manager
                .begin_workspace_remove_admission(&lease)
                .map_err(Self::map_workspace_capability_remove_error)?;
            let workspace = crate::workspace_registry::WorkspaceRegistry::new(self).get(id)?;
            if self
                .workspace_remove_owners(product, &workspace, &lease)?
                .is_busy()
            {
                drop(removal);
                return Err(crate::workspace_registry::WORKSPACE_IN_USE.into());
            }
            (lease, removal)
        };
        if let Err(error) = self.capability_manager.drain_workspace_remove(&lease).await {
            drop(removal);
            return Err(Self::map_workspace_capability_remove_error(error));
        }
        let result = self.remove_workspace_registry_coordinated(product, id);
        drop(removal);
        result
    }

    /// 将 capability Remove 的安全错误映射为已有 Local Workspace 错误 surface。
    fn map_workspace_capability_remove_error(
        error: crate::workspace_capability::WorkspaceCapabilityError,
    ) -> String {
        match error.code {
            WorkspaceCapabilityErrorCode::Busy => {
                crate::workspace_registry::WORKSPACE_IN_USE.into()
            }
            _ => serde_json::to_value(error.code)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "WORKSPACE_CAPABILITY_CONTRACT_ERROR".into()),
        }
    }

    /// Claim 检查与 Registry 删除共用 operation mutex，且只在 capability ownership 已释放后调用。
    fn remove_workspace_registry_coordinated(
        &self,
        product: &AgentProductService,
        id: &str,
    ) -> Result<Workspace, String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        let lease = WorkspaceResolver::new(self).resolve(id)?;
        let workspace = crate::workspace_registry::WorkspaceRegistry::new(self).get(id)?;
        #[cfg(test)]
        if let Some(hook) = self.workspace_remove_hook.lock().unwrap().clone() {
            hook();
        }
        if self
            .workspace_remove_owners(product, &workspace, &lease)?
            .is_busy()
        {
            return Err(crate::workspace_registry::WORKSPACE_IN_USE.into());
        }
        let mut removed = None;
        self.mutate_workspace_registry_locked(|workspaces| {
            let index = workspaces
                .iter()
                .position(|workspace| workspace.id == id)
                .ok_or_else(|| String::from(crate::workspace_registry::WORKSPACE_NOT_FOUND))?;
            removed = Some(workspaces.remove(index));
            Ok(())
        })?;
        removed.ok_or_else(|| "workspace removal made no entry".into())
    }

    /// Host shutdown 通过同一 Manager 收敛 RuntimeSlot；失败时保留 Provider ownership。
    pub(crate) async fn shutdown_capability_runtimes(&self) -> Result<(), String> {
        self.capability_manager
            .shutdown_runtimes()
            .await
            .map_err(|error| {
                serde_json::to_value(error.code)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "WORKSPACE_CAPABILITY_CONTRACT_ERROR".into())
            })
    }

    /// Start 的线性化点：在同一 operation mutex 内解析 Lease，并原子提交 Execution 与 Claim。
    pub(crate) fn create_workspace_start(
        &self,
        store: &StateStore,
        creation: WorkspaceStartCreation,
    ) -> Result<CreateOutcome, String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        let lease = WorkspaceResolver::new(self).resolve(&creation.workspace_id)?;
        if self
            .capability_manager
            .workspace_remove_admission_active(&lease)
        {
            return Err(crate::workspace_registry::WORKSPACE_IN_USE.into());
        }
        let snapshot = WorkspaceSnapshot {
            id: lease.workspace_id,
            root: lease.canonical_root.to_string_lossy().into_owned(),
            generation: lease.generation,
        };
        #[cfg(test)]
        if let Some(hook) = self.workspace_start_hook.lock().unwrap().clone() {
            hook();
        }
        store.product_create_fresh_with_work_blocking(
            creation.execution_id,
            creation.agent_id,
            creation.request_key,
            creation.prompt,
            creation.workspace_id,
            snapshot,
            creation.work,
            creation.now,
        )
    }

    /// Remote settings do not affect Serena discovery or its running process.
    pub fn replace_remote_access(
        &self,
        remote: crate::remote::RemoteAccessConfig,
    ) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        #[cfg(test)]
        if let Some(hook) = self.remote_save_hook.lock().unwrap().clone() {
            hook()?;
        }
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        let mut next = runtime.config.clone();
        next.remote_access = remote;
        next.validate()?;
        config::save(&self.paths.config_file, &next)?;
        runtime.config = next;
        Ok(())
    }

    pub fn replace_config(&self, next: ManagerConfig) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        next.validate()?;
        let installation = discovery::detect(&next, &self.paths);
        if next.serena_path.is_some() && installation.state != InstallationState::Standard {
            return Err(installation
                .error
                .unwrap_or_else(|| "需要兼容的 官方 Serena。".into()));
        }
        let installation = Some(installation);
        config::save(&self.paths.config_file, &next)?;
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        runtime.config = next;
        runtime.installation = installation;
        Ok(())
    }

    pub fn start(&self) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        self.start_unlocked()
    }

    pub fn start_automatically_if(&self, allowed: impl FnOnce() -> bool) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        let auto_start_enabled = self
            .runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .config
            .auto_start_server;
        if !auto_start_enabled || !allowed() {
            return Ok(());
        }
        self.start_unlocked()
    }

    fn start_unlocked(&self) -> Result<(), String> {
        self.refresh_process_status();
        let config = {
            let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
            if runtime.process.is_some() {
                return Err("Serena 已经在运行。".into());
            }
            runtime.status = ServerStatus::Starting;
            runtime.last_error = None;
            runtime.config.clone()
        };
        // Re-probe on every start: a cached detection is not a launch guarantee.
        let installation = discovery::detect(&config, &self.paths);
        self.commit_detection(&config, Some(installation.clone()));
        let preflight = validate_start(&installation, || self.detect_git(), &config);
        if let Err(error) = preflight {
            self.set_error(&error);
            return Err(error);
        }

        if let Err(error) = ensure_port_available(config.port) {
            self.set_error(&error);
            return Err(error);
        }
        self.paths
            .prepare_serena(config.dashboard_enabled)
            .inspect_err(|e| self.set_error(e))?;
        let mut command = start_command(&installation.path, &config, &self.paths.broker_context());
        command
            .env("SERENA_HOME", self.paths.serena_home())
            .env("FASTMCP_JSON_RESPONSE", "false")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            let message = format!("无法启动 Serena：{error}");
            self.set_error(&message);
            message
        })?;
        #[cfg(windows)]
        let job = contain_process(&child).map_err(|error| {
            if terminate_managed_process(&mut child).is_ok() || child.kill().is_ok() {
                let _ = child.wait();
            }
            let message = format!("无法绑定 Serena 进程生命周期：{error}");
            self.set_error(&message);
            message
        })?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        spawn_log_reader(
            stdout,
            "stdout",
            self.paths.serena_log.clone(),
            self.dashboard_url.clone(),
        );
        spawn_log_reader(
            stderr,
            "stderr",
            self.paths.serena_log.clone(),
            self.dashboard_url.clone(),
        );

        {
            let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
            runtime.process = Some(ManagedProcess {
                child,
                #[cfg(windows)]
                _job: job,
                port: config.port,
                dashboard_enabled: config.dashboard_enabled,
                installation: installation.clone(),
            });
        }
        logs::append(
            &self.paths.app_log,
            "app",
            &format!("Serena 启动中，端口 {}", config.port),
        );

        let deadline = Instant::now() + Duration::from_secs(12);
        while Instant::now() < deadline {
            thread::sleep(Duration::from_millis(150));
            self.refresh_process_status();
            let status = self
                .runtime
                .lock()
                .expect("supervisor mutex poisoned")
                .status;
            if status == ServerStatus::Error {
                return Err(self
                    .runtime
                    .lock()
                    .expect("supervisor mutex poisoned")
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "Serena 启动失败。".into()));
            }
            if can_connect(config.port) {
                thread::sleep(Duration::from_millis(150));
                self.refresh_process_status();
                let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
                if runtime.process.is_some() {
                    runtime.status = ServerStatus::Running;
                    logs::append(&self.paths.app_log, "app", "Serena MCP 端口已就绪");
                    return Ok(());
                }
            }
        }

        let mut message = format!("Serena 启动超时：12 秒内未监听 127.0.0.1:{}。", config.port);
        if let Err(stop_error) = self.stop_process(false) {
            message.push_str(&format!(" 同时无法终止进程：{stop_error}"));
        }
        self.set_error(&message);
        Err(message)
    }

    pub fn stop(&self) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        self.stop_process(true)
    }

    pub fn restart(&self) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        self.stop_process(true)?;
        self.start_unlocked()
    }

    fn stop_process(&self, user_requested: bool) -> Result<(), String> {
        let process = self
            .runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .process
            .take();
        let Some(mut process) = process else {
            let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
            if user_requested {
                runtime.status = ServerStatus::Stopped;
                runtime.last_error = None;
            }
            return Ok(());
        };

        if let Err(kill_error) = terminate_managed_process(&mut process.child) {
            match process.child.try_wait() {
                Ok(Some(_)) => {}
                _ => {
                    let message = format!("无法停止 Serena：{kill_error}");
                    let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
                    runtime.process = Some(process);
                    runtime.status = ServerStatus::Error;
                    runtime.last_error = Some(message.clone());
                    logs::append(&self.paths.app_log, "app", &message);
                    return Err(message);
                }
            }
        }
        let _ = process.child.wait();
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        runtime.status = if user_requested {
            ServerStatus::Stopped
        } else {
            ServerStatus::Error
        };
        if user_requested {
            runtime.last_error = None;
            logs::append(&self.paths.app_log, "app", "Serena 已停止");
        }
        Ok(())
    }

    fn refresh_process_status(&self) {
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        let process_status = runtime
            .process
            .as_mut()
            .map(|process| process.child.try_wait());
        match process_status {
            Some(Ok(Some(status))) => {
                runtime.process = None;
                runtime.status = ServerStatus::Error;
                let message = exit_message(status);
                runtime.last_error = Some(message.clone());
                logs::append(&self.paths.app_log, "app", &message);
            }
            Some(Err(error)) => {
                runtime.status = ServerStatus::Error;
                let message = format!("无法读取 Serena 进程状态：{error}");
                runtime.last_error = Some(message.clone());
                logs::append(&self.paths.app_log, "app", &message);
            }
            _ => {}
        }
        if runtime.status == ServerStatus::Running
            && let Some(port) = runtime.process.as_ref().map(|process| process.port)
            && !can_connect(port)
        {
            runtime.status = ServerStatus::Error;
            let message = format!("Serena 进程仍存在，但 127.0.0.1:{port} 已无法连接。");
            runtime.last_error = Some(message.clone());
            logs::append(&self.paths.app_log, "app", &message);
        }
    }

    fn set_error(&self, message: &str) {
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        runtime.status = ServerStatus::Error;
        runtime.last_error = Some(message.to_string());
        logs::append(&self.paths.app_log, "app", message);
    }
}

#[cfg(windows)]
pub(crate) fn contain_process(child: &Child) -> std::io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    // The unnamed handle is not inheritable. Windows closes it even on abort or
    // TerminateProcess, killing this managed process and its descendants.
    unsafe {
        let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if raw.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = OwnedHandle::from_raw_handle(raw);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        ) == 0
            || AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }
}

#[cfg(windows)]
pub(crate) fn terminate_managed_process(child: &mut Child) -> Result<(), String> {
    let mut command = hidden_command("taskkill.exe");
    command.args(["/PID", &child.id().to_string(), "/T", "/F"]);
    let output = run_with_timeout(command, Duration::from_secs(10), "终止 Serena 进程树")?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        Err(format!("taskkill 失败（{}）：{detail}", output.status))
    }
}

/// 终止调用方独占的 Windows Job Object 及其进程树。
#[cfg(windows)]
pub(crate) fn terminate_managed_job(
    job: &std::os::windows::io::OwnedHandle,
) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::TerminateJobObject;

    // SAFETY: 调用方持有该 Job 的唯一 OwnedHandle；只影响被分配到该 Job 的进程树。
    if unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn terminate_managed_process(child: &mut Child) -> Result<(), String> {
    child.kill().map_err(|error| error.to_string())
}

fn validate_start(
    installation: &SerenaInstallation,
    detect_git: impl FnOnce() -> GitInstallation,
    config: &ManagerConfig,
) -> Result<(), String> {
    if installation.state != InstallationState::Standard {
        return Err(installation
            .error
            .clone()
            .unwrap_or_else(|| "请先安装 官方 Serena。".into()));
    }
    let git = detect_git();
    if !git.available {
        return Err(git.error.unwrap_or_else(|| "Git 不可用。".into()));
    }
    config.validate()
}

fn start_command(path: &Path, config: &ManagerConfig, context: &Path) -> Command {
    let mut command = hidden_command(path);
    command
        .args(["start-mcp-server", "--context"])
        .arg(context)
        .args([
            "--transport",
            "streamable-http",
            "--host",
            "127.0.0.1",
            "--port",
            &config.port.to_string(),
            "--open-web-dashboard",
            if config.open_dashboard_on_launch {
                "true"
            } else {
                "false"
            },
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

pub fn find_executable(name: &str) -> Option<PathBuf> {
    let executable_name = if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    env::var_os("PATH")
        .into_iter()
        .flat_map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join(&executable_name))
        .find(|path| path.is_file())
}

pub fn user_local_candidate(name: &str) -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .map(|home| home.join(".local").join("bin").join(filename))
        .filter(|path| path.is_file())
}

pub fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

pub fn run_with_timeout(
    mut command: Command,
    timeout: Duration,
    action: &str,
) -> Result<CapturedOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动{action}进程：{error}"))?;
    let stdout = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let stderr = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                join_reader(stdout);
                join_reader(stderr);
                return Err(format!("{action}超时，已终止本次进程。"));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                join_reader(stdout);
                join_reader(stderr);
                return Err(format!("无法读取{action}进程状态：{error}"));
            }
        }
    };

    Ok(CapturedOutput {
        status,
        stdout: join_reader(stdout),
        stderr: join_reader(stderr),
    })
}

fn join_reader(reader: Option<thread::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn ensure_port_available(port: u16) -> Result<(), String> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map(drop)
        .map_err(|error| format!("端口 {port} 已被占用或无法绑定：{error}"))
}

fn can_connect(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        Duration::from_millis(120),
    )
    .is_ok()
}

fn spawn_log_reader<R: Read + Send + 'static>(
    reader: Option<R>,
    source: &'static str,
    log_path: PathBuf,
    dashboard_url: Arc<Mutex<String>>,
) {
    let Some(reader) = reader else { return };
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            logs::append(&log_path, source, &line);
            if let Some(url) = extract_dashboard_url(&line) {
                *dashboard_url.lock().expect("dashboard URL mutex poisoned") = url;
            }
        }
    });
}

fn extract_dashboard_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .map(|part| part.trim_matches(|character: char| "()[]{}<>,;\"'".contains(character)))
        .find(|part| {
            (part.starts_with("http://127.0.0.1:") || part.starts_with("http://localhost:"))
                && part.contains("/dashboard/index.html")
        })
        .map(ToOwned::to_owned)
}

fn exit_message(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| format!("Serena 意外退出，退出码 {code}。"))
        .unwrap_or_else(|| "Serena 被外部终止。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::InstallationSource;

    /// 建立两个独立 Workspace，供 Guard 生命周期与 Remove 排斥回归使用。
    fn workspace_write_guard_fixture() -> (tempfile::TempDir, SupervisorState, Workspace, Workspace)
    {
        let directory = tempfile::tempdir().unwrap();
        let first_root = directory.path().join("first");
        let second_root = directory.path().join("second");
        std::fs::create_dir_all(&first_root).unwrap();
        std::fs::create_dir_all(&second_root).unwrap();
        let first = Workspace {
            id: "first".into(),
            name: "First".into(),
            root: std::fs::canonicalize(&first_root).unwrap(),
            generation: 11,
        };
        let second = Workspace {
            id: "second".into(),
            name: "Second".into(),
            root: std::fs::canonicalize(&second_root).unwrap(),
            generation: 23,
        };
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        config::save(
            &paths.config_file,
            &ManagerConfig {
                workspace_registry_revision: 7,
                workspaces: vec![first.clone(), second.clone()],
                ..ManagerConfig::default()
            },
        )
        .unwrap();
        (
            directory,
            SupervisorState::new(paths).unwrap(),
            first,
            second,
        )
    }

    #[test]
    fn supervisor_registers_production_capabilities_in_frozen_order() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config").join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs").join("app.log"),
            serena_log: directory.path().join("logs").join("serena.log"),
        };
        std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        config::save(&paths.config_file, &ManagerConfig::default()).unwrap();

        let supervisor = SupervisorState::new(paths).unwrap();
        let providers = supervisor.capability_manager.providers();

        assert_eq!(providers.len(), 4);
        assert_eq!(providers[0].descriptor().provider_id.as_str(), "serena");
        assert_eq!(
            providers[0].descriptor().tool_names,
            [
                "source_symbols_overview",
                "source_find_symbol",
                "source_find_references"
            ]
        );
        assert_eq!(providers[1].descriptor().provider_id.as_str(), "source");
        assert_eq!(providers[2].descriptor().provider_id.as_str(), "git");
        assert_eq!(providers[3].descriptor().provider_id.as_str(), "codegraph");
        assert_eq!(
            providers[3].descriptor().runtime_model,
            crate::workspace_capability::CapabilityRuntimeModel::WorkspaceScopedProcess
        );
    }

    #[test]
    /// 同一 Workspace 可并发持有多个 Guard，Drop 必须精确释放并清理最后一个条目。
    fn workspace_write_guards_are_per_workspace_raii_refcounts() {
        let (_directory, supervisor, first, second) = workspace_write_guard_fixture();
        let (first_lease, first_guard) =
            supervisor.resolve_workspace_write_guard(&first.id).unwrap();
        assert_eq!(first_lease.workspace_id, first.id);
        assert_eq!(first_lease.generation, first.generation);
        assert_eq!(first_lease.canonical_root, first.root);
        assert_eq!(
            crate::workspace_registry::WorkspaceRegistry::new(&supervisor)
                .get(&first.id)
                .unwrap(),
            first
        );
        assert_eq!(
            supervisor.workspace_write_guard_count_for_test(&first_lease),
            1
        );

        let (same_lease, second_guard) =
            supervisor.resolve_workspace_write_guard(&first.id).unwrap();
        assert_eq!(same_lease, first_lease);
        assert_eq!(
            supervisor.workspace_write_guard_count_for_test(&first_lease),
            2
        );

        let (second_workspace_lease, other_workspace_guard) = supervisor
            .resolve_workspace_write_guard(&second.id)
            .unwrap();
        assert_eq!(
            supervisor.workspace_write_guard_count_for_test(&second_workspace_lease),
            1
        );
        assert_eq!(
            supervisor.workspace_write_guard_count_for_test(&first_lease),
            2
        );

        drop(first_guard);
        assert_eq!(
            supervisor.workspace_write_guard_count_for_test(&first_lease),
            1
        );
        drop(second_guard);
        assert_eq!(
            supervisor.workspace_write_guard_count_for_test(&first_lease),
            0
        );
        drop(other_workspace_guard);
        assert_eq!(
            supervisor.workspace_write_guard_count_for_test(&second_workspace_lease),
            0
        );
    }

    #[test]
    /// Registry 的 rename、reorder 与 Desktop selection 不改变目标 Guard 的 Lease 身份。
    fn workspace_write_guard_does_not_block_non_remove_registry_changes() {
        let (_directory, supervisor, first, second) = workspace_write_guard_fixture();
        let (lease, guard) = supervisor.resolve_workspace_write_guard(&first.id).unwrap();
        let registry = crate::workspace_registry::WorkspaceRegistry::new(&supervisor);

        registry.rename(&first.id, "Renamed First".into()).unwrap();
        registry
            .reorder(vec![second.id.clone(), first.id.clone()])
            .unwrap();
        assert_eq!(
            supervisor.select_desktop_workspace(&second.id).unwrap(),
            second
        );
        assert_eq!(
            WorkspaceResolver::new(&supervisor)
                .resolve(&first.id)
                .unwrap(),
            lease
        );

        drop(guard);
        assert_eq!(supervisor.workspace_write_guard_count_for_test(&lease), 0);
        assert_eq!(
            supervisor
                .resolve_workspace_write_guard("missing-workspace")
                .map(|_| ()),
            Err(crate::workspace_registry::WORKSPACE_NOT_FOUND.into())
        );
    }

    #[tokio::test]
    /// Guard 只排斥其自身 Workspace 的 Remove，不影响其他 Workspace。
    async fn workspace_write_guard_does_not_block_other_workspace_remove() {
        let (directory, supervisor, first, second) = workspace_write_guard_fixture();
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = AgentProductService::new(store);
        let (_lease, first_guard) = supervisor.resolve_workspace_write_guard(&first.id).unwrap();

        assert_eq!(
            supervisor
                .remove_workspace_coordinated(&product, &second.id)
                .await
                .unwrap(),
            second
        );
        drop(first_guard);
    }

    #[test]
    /// Guard 获取与 Remove 竞争同一个 operation 边界，只允许其中一方先线性化。
    fn workspace_write_guard_and_remove_are_linearized() {
        use std::sync::{Barrier, mpsc};

        let (directory, supervisor, first, _second) = workspace_write_guard_fixture();
        let store = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(StateStore::open(directory.path().join("agent-state")))
            .unwrap();
        let supervisor = Arc::new(supervisor);
        let product = Arc::new(AgentProductService::new(store));
        let barrier = Arc::new(Barrier::new(3));
        let (acquire_result_tx, acquire_result_rx) = mpsc::channel();
        let (remove_result_tx, remove_result_rx) = mpsc::channel();
        let (release_guard_tx, release_guard_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let acquire_supervisor = Arc::clone(&supervisor);
            let acquire_id = first.id.clone();
            let acquire_barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                acquire_barrier.wait();
                match acquire_supervisor.resolve_workspace_write_guard(&acquire_id) {
                    Ok((_lease, guard)) => {
                        acquire_result_tx.send(Ok(())).unwrap();
                        release_guard_rx.recv().unwrap();
                        drop(guard);
                    }
                    Err(error) => acquire_result_tx.send(Err(error)).unwrap(),
                }
            });
            let remove_supervisor = Arc::clone(&supervisor);
            let remove_product = Arc::clone(&product);
            let remove_id = first.id.clone();
            let remove_barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                remove_barrier.wait();
                let result = tokio::runtime::Runtime::new().unwrap().block_on(
                    remove_supervisor
                        .remove_workspace_coordinated(remove_product.as_ref(), &remove_id),
                );
                remove_result_tx.send(result).unwrap();
            });
            barrier.wait();

            let acquire_result = acquire_result_rx.recv().unwrap();
            let remove_result = remove_result_rx.recv().unwrap();
            let guard_active = acquire_result.is_ok();
            match (acquire_result, remove_result) {
                (Ok(()), Err(error)) => {
                    assert_eq!(error, crate::workspace_registry::WORKSPACE_IN_USE)
                }
                (Err(error), Ok(removed)) => {
                    assert!(matches!(
                        error.as_str(),
                        crate::workspace_registry::WORKSPACE_IN_USE
                            | crate::workspace_registry::WORKSPACE_NOT_FOUND
                    ));
                    assert_eq!(removed, first);
                }
                (acquire, remove) => panic!(
                    "Guard 与 Remove 必须只允许一方成功：acquire={acquire:?}, remove={remove:?}"
                ),
            }
            if guard_active {
                release_guard_tx.send(()).unwrap();
            }
        });
    }

    #[test]
    #[cfg(windows)]
    fn job_owner_fixture() {
        let Some(signal) = std::env::var_os("SERENA_JOB_TEST_SIGNAL") else {
            return;
        };
        let mut child = hidden_command("ping.exe")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let _job = contain_process(&child).unwrap();
        std::fs::write(signal, child.id().to_string()).unwrap();
        child.wait().unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn managed_process_dies_when_owner_is_terminated() {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };
        let dir = tempfile::tempdir().unwrap();
        let signal = dir.path().join("pid");
        let mut owner = hidden_command(std::env::current_exe().unwrap())
            .args(["--exact", "serena::tests::job_owner_fixture", "--nocapture"])
            .env("SERENA_JOB_TEST_SIGNAL", &signal)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !signal.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if !signal.exists() {
            let _ = owner.kill();
            let _ = owner.wait();
            panic!("job fixture did not become ready");
        }
        let pid = std::fs::read_to_string(signal).unwrap().parse().unwrap();
        // Hold the process handle before killing the owner to avoid PID reuse.
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        owner.kill().unwrap(); // No Rust destructor runs in the owner.
        owner.wait().unwrap();
        assert!(!raw.is_null());
        let child = unsafe { OwnedHandle::from_raw_handle(raw) };
        assert_eq!(
            unsafe { WaitForSingleObject(child.as_raw_handle(), 5000) },
            0
        );
    }

    #[test]
    #[cfg(windows)]
    fn terminating_a_managed_job_leaves_another_slot_alive() {
        let mut first = hidden_command("ping.exe")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let first_job = contain_process(&first).unwrap();
        let mut second = hidden_command("ping.exe")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let second_job = contain_process(&second).unwrap();

        terminate_managed_job(&first_job).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while first.try_wait().unwrap().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }

        assert!(first.try_wait().unwrap().is_some());
        assert!(second.try_wait().unwrap().is_none());
        terminate_managed_job(&second_job).unwrap();
        second.wait().unwrap();
    }

    #[test]
    fn startup_checks_supported_version_then_git_then_configuration() {
        let mut installation = SerenaInstallation {
            state: InstallationState::Invalid,
            source: InstallationSource::Path,
            path: "standard.exe".into(),
            version: "1.7.1".into(),
            context: None,
            error: Some("incompatible".into()),
        };
        assert_eq!(
            validate_start(
                &installation,
                || panic!("incompatible Serena must fail before Git"),
                &ManagerConfig::default()
            )
            .unwrap_err(),
            "incompatible"
        );
        installation.state = InstallationState::Standard;
        let config = ManagerConfig {
            port: 80,
            ..ManagerConfig::default()
        };
        assert!(
            validate_start(&installation, GitInstallation::default, &config)
                .unwrap_err()
                .contains("Git")
        );
        assert!(
            validate_start(
                &installation,
                || GitInstallation {
                    available: true,
                    ..GitInstallation::default()
                },
                &config
            )
            .unwrap_err()
            .contains("1024")
        );
        assert!(
            validate_start(
                &installation,
                || GitInstallation {
                    available: true,
                    ..GitInstallation::default()
                },
                &ManagerConfig::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn launch_uses_private_context_and_loopback_without_project() {
        let command = start_command(
            Path::new("C:/managed runtime/bin/serena.exe"),
            &ManagerConfig::default(),
            Path::new("C:/runtime/serena-home/broker.yml"),
        );
        assert_eq!(command.get_program(), "C:/managed runtime/bin/serena.exe");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "start-mcp-server",
                "--context",
                "C:/runtime/serena-home/broker.yml",
                "--transport",
                "streamable-http",
                "--host",
                "127.0.0.1",
                "--port",
                "9121",
                "--open-web-dashboard",
                "false"
            ]
        );
    }

    #[test]
    fn extracts_only_loopback_dashboard_url() {
        assert_eq!(
            extract_dashboard_url("INFO Dashboard: http://127.0.0.1:24283/dashboard/index.html"),
            Some("http://127.0.0.1:24283/dashboard/index.html".into())
        );
        assert_eq!(
            extract_dashboard_url("http://example.com/dashboard/index.html"),
            None
        );
    }

    #[test]
    fn background_start_rechecks_current_conditions() {
        let directory = tempfile::tempdir().unwrap();
        let config_file = directory.path().join("config.json");
        let config = ManagerConfig {
            auto_start_server: false,
            ..ManagerConfig::default()
        };
        config::save(&config_file, &config).unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file,
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs").join("app.log"),
            serena_log: directory.path().join("logs").join("serena.log"),
        };
        let supervisor = SupervisorState::new(paths).unwrap();

        assert!(supervisor.start_automatically_if(|| true).is_ok());
        assert_eq!(supervisor.snapshot().server_status, ServerStatus::Stopped);
    }

    #[test]
    fn stale_detection_does_not_overwrite_newer_config_state() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs").join("app.log"),
            serena_log: directory.path().join("logs").join("serena.log"),
        };
        let supervisor = SupervisorState::new(paths).unwrap();
        let stale_config = ManagerConfig::default();
        let current_installation = SerenaInstallation {
            path: PathBuf::from("current-serena.exe"),
            version: "current".into(),
            state: InstallationState::Standard,
            source: InstallationSource::External,
            context: Some(discovery::SERENA_CONTEXT.into()),
            error: None,
        };
        {
            let mut runtime = supervisor.runtime.lock().unwrap();
            runtime.config.port = 9122;
            runtime.installation = Some(current_installation.clone());
        }

        let result = supervisor.commit_detection(
            &stale_config,
            Some(SerenaInstallation {
                path: PathBuf::from("stale-serena.exe"),
                version: "stale".into(),
                state: InstallationState::Standard,
                source: InstallationSource::External,
                context: Some(discovery::SERENA_CONTEXT.into()),
                error: None,
            }),
        );

        assert_eq!(result.unwrap().path, current_installation.path);
        assert_eq!(
            supervisor.snapshot().installation.unwrap().version,
            current_installation.version
        );
    }

    #[cfg(windows)]
    #[test]
    fn bounded_command_returns_output() {
        let mut command = hidden_command("cmd.exe");
        command.args(["/D", "/C", "echo Serena"]);
        let output = run_with_timeout(command, Duration::from_secs(3), "测试命令").unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Serena"));
    }

    #[cfg(windows)]
    #[test]
    fn bounded_command_terminates_after_timeout() {
        let mut command = hidden_command("powershell.exe");
        command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 5"]);
        let error = run_with_timeout(command, Duration::from_millis(100), "测试命令").unwrap_err();
        assert!(error.contains("超时"));
    }
}

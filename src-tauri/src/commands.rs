use crate::{
    agent::product::AgentProductService,
    config::ManagerConfig,
    serena::SupervisorState,
    workspace_registry::{WorkspaceRegistry, WorkspaceRegistrySnapshot},
};
use serde::Serialize;
use std::{path::Path, process::Stdio};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    codegraph_version: Option<String>,
    config: ManagerConfig,
    desktop_selected_workspace: Option<crate::config::Workspace>,
    git: crate::discovery::GitInstallation,
    managed_runtime_present: bool,
    installation: Option<crate::serena::SerenaInstallation>,
    active_installation: Option<crate::serena::SerenaInstallation>,
    server_status: crate::serena::ServerStatus,
    managed_process_present: bool,
    active_port: u16,
    endpoint: String,
    dashboard_url: String,
    dashboard_enabled: bool,
    log_directory: String,
    autostart_enabled: Option<bool>,
    autostart_error: Option<String>,
    last_error: Option<String>,
}

pub fn build_app_state(app: &AppHandle, known_autostart: Option<bool>) -> AppState {
    let supervisor = app.state::<std::sync::Arc<SupervisorState>>();
    let snapshot = supervisor.snapshot();
    let (autostart_enabled, autostart_error) = resolve_autostart(known_autostart, || {
        app.autolaunch()
            .is_enabled()
            .map_err(|error| error.to_string())
    });
    let desktop_selected_workspace = resolve_desktop_selected_workspace(&snapshot.config);
    AppState {
        endpoint: format!("http://127.0.0.1:{}/mcp", snapshot.active_port),
        log_directory: supervisor.paths.log_directory.display().to_string(),
        managed_runtime_present: supervisor.paths.runtime_directory.exists(),
        git: snapshot.git,
        codegraph_version: snapshot.codegraph_version,
        config: snapshot.config,
        desktop_selected_workspace,
        installation: snapshot.installation,
        active_installation: snapshot.active_installation,
        server_status: snapshot.server_status,
        managed_process_present: snapshot.managed_process_present,
        active_port: snapshot.active_port,
        dashboard_url: snapshot.dashboard_url,
        dashboard_enabled: snapshot.active_dashboard_enabled,
        autostart_enabled,
        autostart_error,
        last_error: snapshot.last_error,
    }
}

fn resolve_desktop_selected_workspace(config: &ManagerConfig) -> Option<crate::config::Workspace> {
    config
        .desktop_selected_workspace_id
        .as_ref()
        .and_then(|id| {
            config
                .workspaces
                .iter()
                .find(|workspace| workspace.id == *id)
                .cloned()
        })
}

fn resolve_autostart(
    known: Option<bool>,
    read: impl FnOnce() -> Result<bool, String>,
) -> (Option<bool>, Option<String>) {
    match known {
        Some(enabled) => (Some(enabled), None),
        None => match read() {
            Ok(enabled) => (Some(enabled), None),
            Err(error) => (
                None,
                Some(format!("无法读取 Windows 登录自启状态：{error}")),
            ),
        },
    }
}

#[tauri::command]
pub fn get_app_state(app: AppHandle) -> Result<AppState, String> {
    Ok(build_app_state(&app, None))
}

#[tauri::command]
pub fn workspace_list(app: AppHandle) -> WorkspaceRegistrySnapshot {
    let supervisor = app.state::<std::sync::Arc<SupervisorState>>();
    WorkspaceRegistry::new(&supervisor).list()
}

#[tauri::command]
pub fn workspace_get(app: AppHandle, id: String) -> Result<crate::config::Workspace, String> {
    let supervisor = app.state::<std::sync::Arc<SupervisorState>>();
    WorkspaceRegistry::new(&supervisor).get(&id)
}

#[tauri::command]
pub fn workspace_select(app: AppHandle, id: String) -> Result<crate::config::Workspace, String> {
    let supervisor = app.state::<std::sync::Arc<SupervisorState>>();
    supervisor.select_desktop_workspace(&id)
}

#[tauri::command]
pub fn workspace_register(
    app: AppHandle,
    root: std::path::PathBuf,
    name: Option<String>,
) -> Result<crate::config::Workspace, String> {
    let supervisor = app.state::<std::sync::Arc<SupervisorState>>();
    WorkspaceRegistry::new(&supervisor).register(root, name)
}

#[tauri::command]
pub fn workspace_rename(
    app: AppHandle,
    id: String,
    name: String,
) -> Result<crate::config::Workspace, String> {
    let supervisor = app.state::<std::sync::Arc<SupervisorState>>();
    WorkspaceRegistry::new(&supervisor).rename(&id, name)
}

#[tauri::command]
pub fn workspace_reorder(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<WorkspaceRegistrySnapshot, String> {
    let supervisor = app.state::<std::sync::Arc<SupervisorState>>();
    WorkspaceRegistry::new(&supervisor).reorder(ids)
}

/// 所有本地 Workspace Remove 请求统一进入 Supervisor 的 typed coordination point。
pub(crate) fn remove_workspace(
    supervisor: &SupervisorState,
    product: &AgentProductService,
    id: &str,
) -> Result<crate::config::Workspace, String> {
    supervisor.remove_workspace_coordinated(product, id)
}

#[tauri::command]
pub async fn workspace_remove(
    app: AppHandle,
    id: String,
) -> Result<crate::config::Workspace, String> {
    let supervisor = app.state::<std::sync::Arc<SupervisorState>>();
    let product = app.state::<std::sync::Arc<AgentProductService>>();
    remove_workspace(&supervisor, &product, &id)
}

#[tauri::command]
pub async fn get_codex_version() -> Result<String, String> {
    #[cfg(windows)]
    {
        let executable = crate::agent::codex::discovery::discover().await?;
        let evidence = crate::agent::codex::app_server::managed::verify(executable)
            .await
            .map_err(|e| e.to_string())?;
        Ok(evidence.identity.version)
    }
    #[cfg(not(windows))]
    Err("当前平台不支持本地 Codex Agent".into())
}

#[tauri::command]
pub async fn detect_serena(app: AppHandle) -> Result<AppState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<std::sync::Arc<SupervisorState>>()
            .detect_serena();
        Ok(build_app_state(&app, None))
    })
    .await
    .map_err(|error| format!("检测任务异常结束：{error}"))?
}

#[tauri::command]
pub async fn detect_git(app: AppHandle) -> Result<AppState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<std::sync::Arc<SupervisorState>>().detect_git();
        Ok(build_app_state(&app, None))
    })
    .await
    .map_err(|error| format!("Git 检测任务异常结束：{error}"))?
}

#[tauri::command]
pub async fn install_serena(app: AppHandle) -> Result<AppState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<std::sync::Arc<SupervisorState>>().install()?;
        Ok(build_app_state(&app, None))
    })
    .await
    .map_err(|error| format!("安装任务异常结束：{error}"))?
}

#[tauri::command]
pub async fn repair_serena(app: AppHandle) -> Result<AppState, String> {
    install_serena(app).await
}

pub fn start_impl(app: &AppHandle) -> Result<AppState, String> {
    let broker = crate::mcp::get(app);
    tauri::async_runtime::block_on(async {
        let _m = broker.management.lock().await;
        let _slot = broker.workspace.write().await;
        app.state::<std::sync::Arc<SupervisorState>>().start()
    })?;
    Ok(build_app_state(app, None))
}

#[tauri::command]
pub async fn start_serena(app: AppHandle) -> Result<AppState, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || start_impl(&handle))
        .await
        .map_err(|error| format!("启动任务异常结束：{error}"))?
}

pub fn stop_impl(app: &AppHandle) -> Result<AppState, String> {
    let broker = crate::mcp::get(app);
    tauri::async_runtime::block_on(async {
        let _m = broker.management.lock().await;
        let mut slot = broker.workspace.write().await;
        broker.clear_workspace(&mut slot);
        app.state::<std::sync::Arc<SupervisorState>>().stop()
    })?;
    Ok(build_app_state(app, None))
}

#[tauri::command]
pub async fn stop_serena(app: AppHandle) -> Result<AppState, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || stop_impl(&handle))
        .await
        .map_err(|error| format!("停止任务异常结束：{error}"))?
}

#[tauri::command]
pub async fn restart_serena(app: AppHandle) -> Result<AppState, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        restart_impl(&handle)?;
        Ok(build_app_state(&handle, None))
    })
    .await
    .map_err(|error| format!("重启任务异常结束：{error}"))?
}

#[tauri::command]
pub async fn save_config(app: AppHandle, config: ManagerConfig) -> Result<AppState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let broker = crate::mcp::get(&app);
        tauri::async_runtime::block_on(save_config_impl(&broker, config))?;
        Ok(build_app_state(&app, None))
    })
    .await
    .map_err(|error| format!("保存配置任务异常结束：{error}"))?
}

pub(crate) async fn save_config_impl(
    broker: &crate::mcp::Broker,
    config: ManagerConfig,
) -> Result<(), String> {
    let _m = broker.management.lock().await;
    let existing = broker.config();
    let listener = broker.snapshot().await;
    if listener.running && config.port == listener.port {
        return Err("Serena 端口不能与正在运行的 Broker 端口相同。".into());
    }
    if config.workspaces != existing.workspaces
        || config.broker != existing.broker
        || config.remote_access != existing.remote_access
    {
        return Err("配置已变化，请刷新后重试；项目/Broker 配置使用专用入口".into());
    }
    let mut slot = broker.workspace.write().await;
    // Dashboard preferences apply on the next start; saving them must not stop the backend.
    if config.port != existing.port || config.serena_path != existing.serena_path {
        broker.clear_workspace(&mut slot);
        broker.supervisor.stop()?;
    }
    broker.supervisor.replace_config(config)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<AppState, String> {
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|error| format!("无法启用 Windows 登录自启：{error}"))?;
    } else {
        manager
            .disable()
            .map_err(|error| format!("无法关闭 Windows 登录自启：{error}"))?;
    }
    Ok(build_app_state(&app, Some(enabled)))
}

#[tauri::command]
pub fn open_dashboard(app: AppHandle) -> Result<(), String> {
    let snapshot = app.state::<std::sync::Arc<SupervisorState>>().snapshot();
    if !snapshot.active_dashboard_enabled {
        return Err("Dashboard 已关闭。".into());
    }
    if snapshot.server_status != crate::serena::ServerStatus::Running {
        return Err("Serena 尚未运行，无法打开 Dashboard。".into());
    }
    open_with_system(&snapshot.dashboard_url)
}

#[tauri::command]
pub fn open_log_directory(app: AppHandle) -> Result<(), String> {
    let path = app
        .state::<std::sync::Arc<SupervisorState>>()
        .paths
        .log_directory
        .clone();
    open_with_system(&path)
}

#[tauri::command]
pub fn open_external_url(target: &str) -> Result<(), String> {
    let url = match target {
        "docs" => "https://oraios.github.io/serena/02-usage/010_installation.html",
        "github" => "https://github.com/oraios/serena",
        "serena-desktop" => "https://github.com/lifei6671/serena-desktop",
        "codegraph" => "https://github.com/colbymchenry/codegraph",
        "git" => "https://git-scm.com/downloads",
        "uv" => "https://docs.astral.sh/uv/getting-started/installation/",
        _ => return Err("不支持的外部链接。".into()),
    };
    open_with_system(url)
}

fn open_with_system(target: impl AsRef<Path>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        std::process::Command::new("explorer.exe")
            .arg(target.as_ref())
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法打开 {}：{error}", target.as_ref().display()))
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(target.as_ref())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法打开 {}：{error}", target.as_ref().display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{
            product::AgentProductService,
            store::{StateStore, transactions::product::WorkspaceSnapshot},
        },
        config::{self, AppPaths, ManagerConfig, Workspace},
        serena::WorkspaceRemoveTestOwnerCounts,
        workspace_registry::WORKSPACE_IN_USE,
    };
    use std::{
        fs,
        path::Path,
        sync::{Arc, Mutex, mpsc},
        time::Duration,
    };

    fn remove_fixture() -> (tempfile::TempDir, AppPaths, SupervisorState, Workspace) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("marker.txt"), "preserve").unwrap();
        let workspace = Workspace {
            id: "workspace".into(),
            name: "Workspace".into(),
            root: fs::canonicalize(root).unwrap(),
            generation: 4,
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
                workspace_registry_revision: 8,
                workspaces: vec![workspace.clone()],
                ..ManagerConfig::default()
            },
        )
        .unwrap();
        let supervisor = SupervisorState::new(paths.clone()).unwrap();
        (directory, paths, supervisor, workspace)
    }

    /// 所有 busy owner 都必须在 Registry mutation 前失败，并保留 Registry 与磁盘。
    fn assert_remove_in_use_preserves_entry(
        supervisor: &SupervisorState,
        product: &AgentProductService,
        workspace: &Workspace,
        config_file: &Path,
    ) {
        let before = WorkspaceRegistry::new(supervisor).list();
        let bytes = fs::read(config_file).unwrap();
        assert_eq!(
            remove_workspace(supervisor, product, &workspace.id),
            Err(WORKSPACE_IN_USE.into())
        );
        assert_eq!(WorkspaceRegistry::new(supervisor).list(), before);
        assert_eq!(fs::read(config_file).unwrap(), bytes);
        assert_eq!(
            fs::read(workspace.root.join("marker.txt")).unwrap(),
            b"preserve"
        );
        assert!(workspace.root.join(".git").is_dir());
    }

    #[tokio::test]
    async fn remove_workspace_rejects_any_claim_without_changing_registry_or_files() {
        let (directory, paths, supervisor, workspace) = remove_fixture();
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = AgentProductService::new(store.clone());
        store
            .product_create_fresh(
                "execution".into(),
                "agent".into(),
                "key".into(),
                "prompt".into(),
                workspace.id.clone(),
                Some(WorkspaceSnapshot {
                    id: workspace.id.clone(),
                    root: workspace.root.to_string_lossy().into_owned(),
                    generation: workspace.generation,
                }),
                1,
            )
            .await
            .unwrap();
        assert_remove_in_use_preserves_entry(&supervisor, &product, &workspace, &paths.config_file);
    }

    #[tokio::test]
    async fn remove_workspace_rejects_simulated_future_write_guard_without_removing() {
        let (directory, paths, supervisor, workspace) = remove_fixture();
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = AgentProductService::new(store);
        *supervisor.workspace_remove_test_owners.lock().unwrap() = WorkspaceRemoveTestOwnerCounts {
            future_write_guard_count: 1,
            future_runtime_slot_ref_count: 0,
        };

        assert_remove_in_use_preserves_entry(&supervisor, &product, &workspace, &paths.config_file);
    }

    #[tokio::test]
    async fn remove_workspace_rejects_simulated_future_runtime_slot_ref_without_removing() {
        let (directory, paths, supervisor, workspace) = remove_fixture();
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = AgentProductService::new(store);
        *supervisor.workspace_remove_test_owners.lock().unwrap() = WorkspaceRemoveTestOwnerCounts {
            future_write_guard_count: 0,
            future_runtime_slot_ref_count: 1,
        };

        assert_remove_in_use_preserves_entry(&supervisor, &product, &workspace, &paths.config_file);
    }

    #[tokio::test]
    async fn remove_workspace_rejects_typed_check_errors_without_removing() {
        let (directory, paths, supervisor, workspace) = remove_fixture();
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = AgentProductService::new(store);
        let before = WorkspaceRegistry::new(&supervisor).list();
        let bytes = fs::read(&paths.config_file).unwrap();
        rusqlite::Connection::open(directory.path().join("agent-state/agent-state.db"))
            .unwrap()
            .execute_batch("DROP TABLE workspace_claims")
            .unwrap();

        let error = remove_workspace(&supervisor, &product, &workspace.id).unwrap_err();
        assert_ne!(error, WORKSPACE_IN_USE);
        assert_eq!(WorkspaceRegistry::new(&supervisor).list(), before);
        assert_eq!(fs::read(paths.config_file).unwrap(), bytes);
        assert_eq!(
            fs::read(workspace.root.join("marker.txt")).unwrap(),
            b"preserve"
        );
        assert!(workspace.root.join(".git").is_dir());
    }

    #[tokio::test]
    async fn remove_workspace_removes_idle_registry_entry_without_removing_files() {
        let (directory, _paths, supervisor, workspace) = remove_fixture();
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = AgentProductService::new(store);

        assert_eq!(
            remove_workspace(&supervisor, &product, &workspace.id).unwrap(),
            workspace
        );
        assert_eq!(
            WorkspaceRegistry::new(&supervisor).get("workspace"),
            Err(crate::workspace_registry::WORKSPACE_NOT_FOUND.into())
        );
        assert_eq!(
            fs::read(workspace.root.join("marker.txt")).unwrap(),
            b"preserve"
        );
        assert!(workspace.root.join(".git").is_dir());
    }

    #[tokio::test]
    async fn remove_workspace_holds_operation_mutex_across_typed_check_and_registry_delete() {
        let (directory, _paths, supervisor, workspace) = remove_fixture();
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = AgentProductService::new(store);
        let supervisor = Arc::new(supervisor);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release = Arc::new(Mutex::new(release_rx));
        let release_for_hook = release.clone();
        *supervisor.workspace_remove_hook.lock().unwrap() = Some(Arc::new(move || {
            entered_tx.send(()).unwrap();
            release_for_hook.lock().unwrap().recv().unwrap();
        }));
        let (remove_done, remove_result) = mpsc::channel();
        let (rename_done, rename_result) = mpsc::channel();

        std::thread::scope(|scope| {
            let removal_supervisor = supervisor.clone();
            let removal_id = workspace.id.clone();
            let removal_product = &product;
            scope.spawn(move || {
                remove_done
                    .send(remove_workspace(
                        &removal_supervisor,
                        removal_product,
                        &removal_id,
                    ))
                    .unwrap();
            });
            entered_rx.recv().unwrap();
            let rename_supervisor = supervisor.clone();
            let rename_id = workspace.id.clone();
            scope.spawn(move || {
                rename_done
                    .send(
                        WorkspaceRegistry::new(&rename_supervisor)
                            .rename(&rename_id, "Renamed".into()),
                    )
                    .unwrap();
            });
            assert!(
                rename_result
                    .recv_timeout(Duration::from_millis(50))
                    .is_err()
            );
            release_tx.send(()).unwrap();
            assert_eq!(remove_result.recv().unwrap().unwrap().id, workspace.id);
            assert_eq!(
                rename_result.recv().unwrap(),
                Err(crate::workspace_registry::WORKSPACE_NOT_FOUND.into())
            );
        });
        assert_eq!(
            WorkspaceRegistry::new(&supervisor).get("workspace"),
            Err(crate::workspace_registry::WORKSPACE_NOT_FOUND.into())
        );
    }

    #[test]
    fn autostart_read_failure_is_reported_without_failing_app_state() {
        let (enabled, error) = resolve_autostart(None, || Err("registry unavailable".into()));

        assert_eq!(enabled, None);
        assert_eq!(
            error.as_deref(),
            Some("无法读取 Windows 登录自启状态：registry unavailable")
        );
    }

    #[test]
    fn app_state_desktop_selection_uses_only_the_registry_config() {
        let selected = Workspace {
            id: "selected".into(),
            name: "Selected".into(),
            root: "C:/selected".into(),
            generation: 7,
        };
        let config = ManagerConfig {
            desktop_selected_workspace_id: Some(selected.id.clone()),
            workspaces: vec![selected.clone()],
            ..ManagerConfig::default()
        };

        assert_eq!(resolve_desktop_selected_workspace(&config), Some(selected));
        assert_eq!(
            resolve_desktop_selected_workspace(&ManagerConfig::default()),
            None
        );
    }

    #[test]
    fn known_autostart_value_does_not_read_registry_again() {
        let (enabled, error) = resolve_autostart(Some(true), || {
            panic!("known value must bypass the registry read")
        });

        assert_eq!(enabled, Some(true));
        assert_eq!(error, None);
    }
}

pub fn restart_impl(app: &AppHandle) -> Result<(), String> {
    let broker = crate::mcp::get(app);
    tauri::async_runtime::block_on(restart_serena_impl(&broker))
}

pub(crate) async fn restart_serena_impl(broker: &crate::mcp::Broker) -> Result<(), String> {
    let _m = broker.management.lock().await;
    let mut slot = broker.workspace.write().await;
    let workspace = slot.as_ref().map(|active| active.workspace.clone());
    broker.clear_workspace(&mut slot);
    let supervisor = broker.supervisor.clone();
    tauri::async_runtime::spawn_blocking(move || supervisor.restart())
        .await
        .map_err(|error| format!("重启任务异常结束：{error}"))??;
    // Activation acquires the workspace lock itself. Keep management locked until commit
    // so a concurrent activate/deactivate cannot be overwritten by this restoration.
    drop(slot);
    if let Some(workspace) = workspace {
        let restore = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            Box::pin(broker.activate(&workspace.id, tokio_util::sync::CancellationToken::new())),
        )
        .await
        .map_err(|_| "恢复项目超时".to_owned())
        .and_then(|result| result.map(|_| ()));
        if let Err(error) = restore {
            let message = format!("Serena 已重启，但恢复项目 {} 失败：{error}", workspace.name);
            broker.log(&message);
            return Err(message);
        }
        broker.log(&format!("Serena 重启后已恢复项目 · {}", workspace.name));
    }
    Ok(())
}
pub fn shutdown_impl(app: &AppHandle) -> Result<(), String> {
    let broker = crate::mcp::get(app);
    if let Some((_, token)) = broker.operation.lock().unwrap().as_ref() {
        token.cancel();
    }
    tauri::async_runtime::block_on(async {
        app.state::<std::sync::Arc<crate::agent::product::AgentProductService>>()
            .shutdown()
            .await?;
        let _m = broker.management.lock().await;
        broker.shutdown().await?;
        app.state::<std::sync::Arc<SupervisorState>>().stop()
    })
}
#[tauri::command]
pub async fn get_broker_state(app: AppHandle) -> Result<crate::mcp::Snapshot, String> {
    Ok(crate::mcp::get(&app).snapshot().await)
}
#[tauri::command]
pub async fn workspace_import_serena(app: AppHandle) -> Result<usize, String> {
    let user_home = std::env::var_os("SERENA_HOME")
        .filter(|s| !s.to_string_lossy().trim().is_empty())
        .map(|s| std::path::PathBuf::from(s.to_string_lossy().trim()))
        .map(Ok)
        .unwrap_or_else(|| {
            app.path()
                .home_dir()
                .map(|p| p.join(".serena"))
                .map_err(|e| e.to_string())
        })?;
    let broker = crate::mcp::get(&app);
    let sources = vec![
        user_home.join("serena_config.yml"),
        broker
            .supervisor
            .paths
            .serena_home()
            .join("serena_config.yml"),
    ];
    broker.sync_projects(sources).await
}

#[tauri::command]
pub async fn sync_workspaces(app: AppHandle) -> Result<usize, String> {
    workspace_import_serena(app).await
}
#[tauri::command]
pub async fn activate_workspace(app: AppHandle, id: String) -> Result<(), String> {
    let b = crate::mcp::get(&app);
    b.call_tool(
        "workspace_activate",
        serde_json::json!({"id":id}),
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .map(|_| ())
}
#[tauri::command]
pub async fn deactivate_workspace(app: AppHandle) -> Result<(), String> {
    let b = crate::mcp::get(&app);
    b.call_tool(
        "workspace_deactivate",
        serde_json::json!({}),
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .map(|_| ())
}
#[tauri::command]
pub fn cancel_workspace_operation(app: AppHandle) {
    if let Some((_, token)) = crate::mcp::get(&app).operation.lock().unwrap().as_ref() {
        token.cancel();
    }
}
#[tauri::command]
pub async fn set_broker(
    app: AppHandle,
    enabled: bool,
    port: u16,
    allow_lan: bool,
) -> Result<(), String> {
    set_broker_impl(&crate::mcp::get(&app), enabled, port, allow_lan).await
}

pub(crate) async fn set_broker_impl(
    b: &std::sync::Arc<crate::mcp::Broker>,
    enabled: bool,
    port: u16,
    allow_lan: bool,
) -> Result<(), String> {
    let _m = b.management.lock().await;
    if b.remote.active() {
        return Err("REMOTE_ACCESS_ALREADY_RUNNING: 请先停止远程访问再修改 Broker 配置".into());
    }
    let mut c = b.config();
    c.broker = crate::config::BrokerConfig {
        enabled,
        port,
        allow_lan,
    };
    c.validate()?;
    b.stop().await?;
    b.supervisor.replace_config(c)?;
    if enabled {
        b.start().await?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_mcp_logs(app: AppHandle) -> Vec<String> {
    crate::mcp::get(&app).log_snapshot()
}

#[tauri::command]
pub fn clear_mcp_logs(app: AppHandle) {
    crate::mcp::get(&app).clear_logs();
}

#[tauri::command]
pub async fn download_mcp_logs(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let text = crate::mcp::get(&app).log_snapshot().join("\n");
    tauri::async_runtime::spawn_blocking(move || {
        let name = format!(
            "serena-mcp-{}.log",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let Some(file) = app
            .dialog()
            .file()
            .set_file_name(name)
            .add_filter("日志", &["log"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("写入日志失败：{e}"))?;
        Ok(true)
    })
    .await
    .map_err(|e| format!("导出日志任务失败：{e}"))?
}

#[tauri::command]
pub async fn agent_operation(app: AppHandle, request: serde_json::Value) -> serde_json::Value {
    crate::mcp::get(&app).agent_operation(request).await
}

#[tauri::command]
pub async fn agent_history(
    app: AppHandle,
    before: Option<String>,
    workspace: Option<String>,
) -> Result<crate::agent::product::HistoryPage, String> {
    let broker = crate::mcp::get(&app);
    let product = broker
        .product
        .get()
        .ok_or("BACKEND_UNAVAILABLE: Agent service not initialized")?;
    product.history_page(before, workspace).await
}

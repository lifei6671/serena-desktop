use crate::{config::Workspace, serena::SupervisorState};
use serde::Serialize;

pub(crate) const WORKSPACE_NOT_FOUND: &str = "WORKSPACE_NOT_FOUND";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceRegistrySnapshot {
    pub registry_revision: u64,
    pub workspaces: Vec<Workspace>,
}

pub(crate) struct WorkspaceRegistry<'a> {
    supervisor: &'a SupervisorState,
}

impl<'a> WorkspaceRegistry<'a> {
    pub(crate) fn new(supervisor: &'a SupervisorState) -> Self {
        Self { supervisor }
    }

    pub(crate) fn list(&self) -> WorkspaceRegistrySnapshot {
        let config = self.supervisor.workspace_registry_config();
        WorkspaceRegistrySnapshot {
            registry_revision: config.workspace_registry_revision,
            workspaces: config.workspaces,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Result<Workspace, String> {
        self.list()
            .workspaces
            .into_iter()
            .find(|workspace| workspace.id == id)
            .ok_or_else(|| WORKSPACE_NOT_FOUND.into())
    }

    #[allow(
        dead_code,
        reason = "P2A1-003 provides the serialized mutation foundation before P2A1-006/007/008 consume it"
    )]
    pub(crate) fn mutate(
        &self,
        mutation: impl FnOnce(&mut Vec<Workspace>) -> Result<(), String>,
    ) -> Result<bool, String> {
        self.supervisor.mutate_workspace_registry(mutation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, AppPaths, ManagerConfig};
    use std::{
        fs,
        path::PathBuf,
        sync::{Arc, mpsc},
        thread,
    };

    fn workspace(id: &str, name: &str, root: impl Into<PathBuf>, generation: u64) -> Workspace {
        Workspace {
            id: id.into(),
            name: name.into(),
            root: root.into(),
            generation,
        }
    }

    fn fixture(config: ManagerConfig) -> (tempfile::TempDir, AppPaths, SupervisorState) {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        config::save(&paths.config_file, &config).unwrap();
        let supervisor = SupervisorState::new(paths.clone()).unwrap();
        (directory, paths, supervisor)
    }

    #[test]
    fn list_and_get_preserve_order_generation_and_have_no_serena_or_binding_side_effect() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 41,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/unchanged/first", 3),
            workspace("second", "Second", "C:/unchanged/second", 9),
        ];
        let (_directory, paths, supervisor) = fixture(config.clone());
        assert!(!paths.serena_home().exists());
        let registry = WorkspaceRegistry::new(&supervisor);

        assert_eq!(
            registry.list(),
            WorkspaceRegistrySnapshot {
                registry_revision: 41,
                workspaces: config.workspaces.clone(),
            }
        );
        assert_eq!(registry.get("second").unwrap(), config.workspaces[1]);
        assert_eq!(registry.get("unknown-id").unwrap_err(), WORKSPACE_NOT_FOUND);
        assert_eq!(supervisor.workspace_registry_config(), config);
        assert!(!paths.serena_home().exists());
    }

    #[test]
    fn list_serializes_the_frozen_registry_revision_and_workspace_generation_shape() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 42,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/unchanged/first", 3),
            workspace("second", "Second", "C:/unchanged/second", 9),
        ];
        let (_directory, _paths, supervisor) = fixture(config);

        let value = serde_json::to_value(WorkspaceRegistry::new(&supervisor).list()).unwrap();
        assert_eq!(value["registryRevision"], 42);
        assert!(value.get("registry_revision").is_none());
        let workspaces = value["workspaces"].as_array().unwrap();
        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0]["id"], "first");
        assert_eq!(workspaces[0]["generation"], 3);
        assert_eq!(workspaces[1]["id"], "second");
        assert_eq!(workspaces[1]["generation"], 9);
    }

    #[test]
    fn committed_mutation_persists_and_reopen_observes_one_new_revision() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 8,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![workspace("one", "One", "C:/one", 4)];
        let (_directory, paths, supervisor) = fixture(config);
        let registry = WorkspaceRegistry::new(&supervisor);

        assert!(
            registry
                .mutate(|workspaces| {
                    workspaces.push(workspace("two", "Two", "C:/two", 7));
                    Ok(())
                })
                .unwrap()
        );

        let expected = WorkspaceRegistrySnapshot {
            registry_revision: 9,
            workspaces: vec![
                workspace("one", "One", "C:/one", 4),
                workspace("two", "Two", "C:/two", 7),
            ],
        };
        assert_eq!(registry.list(), expected);
        assert_eq!(
            WorkspaceRegistry::new(&SupervisorState::new(paths).unwrap()).list(),
            expected
        );
    }

    #[test]
    fn no_op_mutation_keeps_revision_and_persisted_bytes() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 12,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![workspace("one", "One", "C:/one", 5)];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let bytes = fs::read(&paths.config_file).unwrap();

        assert!(
            !WorkspaceRegistry::new(&supervisor)
                .mutate(|_| Ok(()))
                .unwrap()
        );
        assert_eq!(
            WorkspaceRegistry::new(&supervisor).list().registry_revision,
            12
        );
        assert_eq!(fs::read(paths.config_file).unwrap(), bytes);
        assert_eq!(supervisor.workspace_registry_config(), config);
    }

    #[cfg(windows)]
    #[test]
    fn persist_failure_keeps_memory_and_existing_config_unchanged() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 13,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![workspace("one", "One", "C:/one", 5)];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let bytes = fs::read(&paths.config_file).unwrap();
        let mut permissions = fs::metadata(&paths.config_file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&paths.config_file, permissions).unwrap();

        let result = WorkspaceRegistry::new(&supervisor).mutate(|workspaces| {
            workspaces.push(workspace("two", "Two", "C:/two", 1));
            Ok(())
        });

        let mut permissions = fs::metadata(&paths.config_file).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&paths.config_file, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(
            WorkspaceRegistry::new(&supervisor).list().registry_revision,
            13
        );
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);
        assert_eq!(
            SupervisorState::new(paths)
                .unwrap()
                .workspace_registry_config(),
            config
        );
    }

    #[test]
    fn concurrent_readers_observe_only_complete_registry_snapshots() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 20,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![workspace("one", "One", "C:/one", 2)];
        let (_directory, _paths, supervisor) = fixture(config);
        let supervisor = Arc::new(supervisor);
        let before = WorkspaceRegistry::new(&supervisor).list();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let writer_supervisor = Arc::clone(&supervisor);
        let writer = thread::spawn(move || {
            WorkspaceRegistry::new(&writer_supervisor).mutate(|workspaces| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                workspaces.push(workspace("two", "Two", "C:/two", 6));
                Ok(())
            })
        });
        entered_rx.recv().unwrap();

        let reader_supervisor = Arc::clone(&supervisor);
        let reader = thread::spawn(move || {
            (0..256)
                .map(|_| WorkspaceRegistry::new(&reader_supervisor).list())
                .collect::<Vec<_>>()
        });
        let observed = reader.join().unwrap();
        assert!(!observed.is_empty());
        assert!(observed.iter().all(|snapshot| snapshot == &before));

        release_tx.send(()).unwrap();
        assert!(writer.join().unwrap().unwrap());
        let after = WorkspaceRegistry::new(&supervisor).list();
        assert_eq!(after.registry_revision, before.registry_revision + 1);
        assert_eq!(after.workspaces.len(), before.workspaces.len() + 1);
    }
}

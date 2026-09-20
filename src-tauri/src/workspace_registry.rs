use crate::{
    config::{Workspace, canonicalize_workspace_root, same_workspace_root_identity},
    serena::SupervisorState,
};
use serde::Serialize;
use std::path::PathBuf;

pub(crate) const WORKSPACE_NOT_FOUND: &str = "WORKSPACE_NOT_FOUND";
pub(crate) const WORKSPACE_IN_USE: &str = "WORKSPACE_IN_USE";
pub(crate) const WORKSPACE_ALREADY_EXISTS: &str = "WORKSPACE_ALREADY_EXISTS";
pub(crate) const WORKSPACE_NAME_INVALID: &str = "WORKSPACE_NAME_INVALID";
const WORKSPACE_ID_PREFIX: &str = "project-";
const WORKSPACE_ID_RETRY_LIMIT: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceImportCandidate {
    pub(crate) root: PathBuf,
    pub(crate) name: String,
}

fn generate_workspace_id(workspaces: &[Workspace]) -> Result<String, String> {
    for _ in 0..WORKSPACE_ID_RETRY_LIMIT {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| String::from("WORKSPACE_ID_RANDOM_FAILED"))?;
        let id = format!("{WORKSPACE_ID_PREFIX}{:032x}", u128::from_le_bytes(bytes));
        if !workspaces.iter().any(|workspace| workspace.id == id) {
            return Ok(id);
        }
    }
    Err("WORKSPACE_ID_GENERATION_FAILED".into())
}

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

    pub(crate) fn register(
        &self,
        root: PathBuf,
        name: Option<String>,
    ) -> Result<Workspace, String> {
        let root = canonicalize_workspace_root(&root).map_err(str::to_owned)?;
        let name = match name {
            Some(name) => name.trim().to_owned(),
            None => root
                .file_name()
                .map(|basename| basename.to_string_lossy().trim().to_owned())
                .unwrap_or_default(),
        };
        if name.is_empty() {
            return Err(WORKSPACE_NAME_INVALID.into());
        }

        let mut registered = None;
        let changed = self.mutate(|workspaces| {
            if workspaces
                .iter()
                .any(|workspace| same_workspace_root_identity(&root, &workspace.root))
            {
                return Err(WORKSPACE_ALREADY_EXISTS.into());
            }

            let workspace = Workspace {
                id: generate_workspace_id(workspaces)?,
                name: name.clone(),
                root: root.clone(),
                generation: 1,
            };
            workspaces.push(workspace.clone());
            registered = Some(workspace);
            Ok(())
        })?;

        if !changed {
            return Err("workspace registration made no change".into());
        }
        registered.ok_or_else(|| "workspace registration made no entry".into())
    }

    pub(crate) fn import_serena(
        &self,
        candidates: &[WorkspaceImportCandidate],
    ) -> Result<usize, String> {
        let mut added = 0;
        self.mutate(|workspaces| {
            for candidate in candidates {
                if workspaces
                    .iter()
                    .any(|workspace| same_workspace_root_identity(&candidate.root, &workspace.root))
                {
                    continue;
                }

                workspaces.push(Workspace {
                    id: generate_workspace_id(workspaces)?,
                    name: candidate.name.clone(),
                    root: candidate.root.clone(),
                    generation: 1,
                });
                added += 1;
            }
            Ok(())
        })?;
        Ok(added)
    }

    pub(crate) fn rename(&self, id: &str, name: String) -> Result<Workspace, String> {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(WORKSPACE_NAME_INVALID.into());
        }

        let mut renamed = None;
        self.mutate(|workspaces| {
            let workspace = workspaces
                .iter_mut()
                .find(|workspace| workspace.id == id)
                .ok_or_else(|| String::from(WORKSPACE_NOT_FOUND))?;
            if workspace.name != name {
                workspace.name.clone_from(&name);
            }
            renamed = Some(workspace.clone());
            Ok(())
        })?;
        renamed.ok_or_else(|| "workspace rename made no entry".into())
    }

    /// 保留 Registry-only 删除语义供兼容回归；生产 Remove 必须经 Supervisor typed coordination。
    #[allow(dead_code)]
    pub(crate) fn remove(&self, id: &str) -> Result<Workspace, String> {
        let mut removed = None;
        self.mutate(|workspaces| {
            let index = workspaces
                .iter()
                .position(|workspace| workspace.id == id)
                .ok_or_else(|| String::from(WORKSPACE_NOT_FOUND))?;
            removed = Some(workspaces.remove(index));
            Ok(())
        })?;
        removed.ok_or_else(|| "workspace removal made no entry".into())
    }

    pub(crate) fn reorder(&self, ids: Vec<String>) -> Result<WorkspaceRegistrySnapshot, String> {
        self.mutate(|workspaces| {
            if ids.len() != workspaces.len() {
                return Err("workspace reorder IDs must be a complete registry permutation".into());
            }

            let mut reordered = Vec::with_capacity(workspaces.len());
            for id in &ids {
                if reordered
                    .iter()
                    .any(|workspace: &Workspace| workspace.id == *id)
                {
                    return Err("workspace reorder IDs must not contain duplicates".into());
                }
                let workspace = workspaces
                    .iter()
                    .find(|workspace| workspace.id == *id)
                    .ok_or_else(|| format!("workspace reorder ID is not registered: {id}"))?;
                reordered.push(workspace.clone());
            }
            *workspaces = reordered;
            Ok(())
        })?;
        Ok(self.list())
    }

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

    fn assert_generated_workspace_id(id: &str) {
        let suffix = id.strip_prefix(WORKSPACE_ID_PREFIX).unwrap();
        assert_eq!(suffix.len(), 32);
        assert!(
            suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        );
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

    #[test]
    fn desktop_selection_persists_without_changing_registry_identity_or_runtime() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 12,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("one", "One", "C:/one", 5),
            workspace("two", "Two", "C:/two", 8),
        ];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let before = supervisor.snapshot();

        assert_eq!(
            supervisor.select_desktop_workspace("two").unwrap(),
            config.workspaces[1]
        );
        assert_eq!(
            supervisor.desktop_selected_workspace(),
            Some(config.workspaces[1].clone())
        );
        let persisted = supervisor.workspace_registry_config();
        assert_eq!(
            persisted.workspace_registry_revision,
            config.workspace_registry_revision
        );
        assert_eq!(persisted.workspaces, config.workspaces);
        assert_eq!(
            persisted.desktop_selected_workspace_id.as_deref(),
            Some("two")
        );
        assert!(!paths.serena_home().exists());
        let after = supervisor.snapshot();
        assert_eq!(after.process_id, before.process_id);
        assert_eq!(after.server_status, before.server_status);
        assert_eq!(after.codegraph_version, before.codegraph_version);

        let restored = SupervisorState::new(paths).unwrap();
        assert_eq!(
            restored.desktop_selected_workspace(),
            Some(config.workspaces[1].clone())
        );
        assert!(!restored.snapshot().managed_process_present);
    }

    #[test]
    fn selecting_the_same_workspace_is_a_true_no_op_and_unknown_id_does_not_mutate() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 12,
            desktop_selected_workspace_id: Some("one".into()),
            ..ManagerConfig::default()
        };
        config.workspaces = vec![workspace("one", "One", "C:/one", 5)];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let bytes = fs::read(&paths.config_file).unwrap();

        assert_eq!(
            supervisor.select_desktop_workspace("one").unwrap(),
            config.workspaces[0]
        );
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);
        assert_eq!(
            supervisor.select_desktop_workspace("missing"),
            Err(WORKSPACE_NOT_FOUND.into())
        );
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);
        assert_eq!(supervisor.workspace_registry_config(), config);
    }

    #[test]
    fn removing_selected_workspace_clears_selection_in_the_same_registry_commit() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 8,
            desktop_selected_workspace_id: Some("second".into()),
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/first", 3),
            workspace("second", "Second", "C:/second", 9),
        ];
        let (_directory, paths, supervisor) = fixture(config.clone());

        assert_eq!(
            WorkspaceRegistry::new(&supervisor)
                .remove("second")
                .unwrap(),
            config.workspaces[1]
        );
        let committed = supervisor.workspace_registry_config();
        assert_eq!(committed.workspace_registry_revision, 9);
        assert_eq!(committed.desktop_selected_workspace_id, None);
        assert_eq!(committed.workspaces, vec![config.workspaces[0].clone()]);
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.config_file).unwrap()).unwrap();
        assert!(saved["desktopSelectedWorkspaceId"].is_null());
        assert_eq!(
            SupervisorState::new(paths)
                .unwrap()
                .desktop_selected_workspace(),
            None
        );
    }

    #[test]
    fn registry_mutations_keep_a_remaining_desktop_selection_resolved_by_id() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 8,
            desktop_selected_workspace_id: Some("second".into()),
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/first", 3),
            workspace("second", "Second", "C:/second", 9),
        ];
        let (_directory, _paths, supervisor) = fixture(config);
        let registry = WorkspaceRegistry::new(&supervisor);

        registry.rename("second", "Renamed".into()).unwrap();
        registry
            .reorder(vec!["second".into(), "first".into()])
            .unwrap();
        registry.remove("first").unwrap();

        assert_eq!(
            supervisor.desktop_selected_workspace(),
            Some(workspace("second", "Renamed", "C:/second", 9))
        );
        assert_eq!(
            supervisor
                .workspace_registry_config()
                .desktop_selected_workspace_id
                .as_deref(),
            Some("second")
        );
    }

    #[cfg(windows)]
    #[test]
    fn desktop_selection_persist_failure_keeps_the_previous_selection() {
        let mut config = ManagerConfig {
            desktop_selected_workspace_id: Some("one".into()),
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("one", "One", "C:/one", 5),
            workspace("two", "Two", "C:/two", 8),
        ];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let bytes = fs::read(&paths.config_file).unwrap();
        let original_permissions = fs::metadata(&paths.config_file).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&paths.config_file, readonly_permissions).unwrap();

        let result = supervisor.select_desktop_workspace("two");

        assert!(result.is_err());
        fs::set_permissions(&paths.config_file, original_permissions).unwrap();
        assert_eq!(supervisor.workspace_registry_config(), config);
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);
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
        let original_permissions = fs::metadata(&paths.config_file).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&paths.config_file, readonly_permissions).unwrap();

        let result = WorkspaceRegistry::new(&supervisor).mutate(|workspaces| {
            workspaces.push(workspace("two", "Two", "C:/two", 1));
            Ok(())
        });

        assert!(result.is_err());
        fs::set_permissions(&paths.config_file, original_permissions).unwrap();
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

    #[test]
    fn register_persists_generation_one_and_a_stable_project_id() {
        let (_directory, paths, supervisor) = fixture(ManagerConfig {
            workspace_registry_revision: 8,
            ..ManagerConfig::default()
        });
        let root = paths.runtime_directory.parent().unwrap().join("registered");
        fs::create_dir(&root).unwrap();

        let registered = WorkspaceRegistry::new(&supervisor)
            .register(root.clone(), Some("  Registered workspace  ".into()))
            .unwrap();

        assert_generated_workspace_id(&registered.id);
        assert_eq!(registered.name, "Registered workspace");
        assert_eq!(registered.root, fs::canonicalize(&root).unwrap());
        assert_eq!(registered.generation, 1);
        assert_eq!(
            WorkspaceRegistry::new(&supervisor).list(),
            WorkspaceRegistrySnapshot {
                registry_revision: 9,
                workspaces: vec![registered.clone()],
            }
        );
        assert_eq!(
            WorkspaceRegistry::new(&SupervisorState::new(paths).unwrap()).list(),
            WorkspaceRegistrySnapshot {
                registry_revision: 9,
                workspaces: vec![registered],
            }
        );
    }

    #[test]
    fn register_generates_distinct_ids_and_does_not_reuse_a_removed_entry_id() {
        let (_directory, paths, supervisor) = fixture(ManagerConfig::default());
        let registry = WorkspaceRegistry::new(&supervisor);
        let first_root = paths.runtime_directory.parent().unwrap().join("first");
        let second_root = paths.runtime_directory.parent().unwrap().join("second");
        let third_root = paths.runtime_directory.parent().unwrap().join("third");
        fs::create_dir(&first_root).unwrap();
        fs::create_dir(&second_root).unwrap();
        fs::create_dir(&third_root).unwrap();

        let first = registry.register(first_root, None).unwrap();
        let second = registry.register(second_root, None).unwrap();
        assert_generated_workspace_id(&first.id);
        assert_generated_workspace_id(&second.id);
        assert_ne!(first.id, second.id);

        assert!(
            registry
                .mutate(|workspaces| {
                    workspaces.retain(|workspace| workspace.id != first.id);
                    Ok(())
                })
                .unwrap()
        );
        let third = registry.register(third_root, None).unwrap();
        assert_generated_workspace_id(&third.id);
        assert_ne!(third.id, first.id);
        assert_ne!(third.id, second.id);
    }

    #[test]
    fn serena_import_appends_only_missing_roots_in_one_revision_and_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let root_a = directory.path().join("a");
        let root_b = directory.path().join("b");
        let root_c = directory.path().join("c");
        let root_d = directory.path().join("d");
        fs::create_dir(&root_a).unwrap();
        fs::create_dir(&root_b).unwrap();
        fs::create_dir(&root_c).unwrap();
        fs::create_dir(&root_d).unwrap();
        let root_a = fs::canonicalize(root_a).unwrap();
        let root_b = fs::canonicalize(root_b).unwrap();
        let root_c = fs::canonicalize(root_c).unwrap();
        let root_d = fs::canonicalize(root_d).unwrap();
        let existing_a = workspace("local-a", "Local A", root_a, 3);
        let existing_b = workspace("local-b", "Local B", root_b.clone(), 7);
        let config = ManagerConfig {
            workspace_registry_revision: 8,
            desktop_selected_workspace_id: Some(existing_b.id.clone()),
            workspaces: vec![existing_a.clone(), existing_b.clone()],
            ..ManagerConfig::default()
        };
        let (_directory, paths, supervisor) = fixture(config.clone());
        let registry = WorkspaceRegistry::new(&supervisor);
        let candidates = vec![
            WorkspaceImportCandidate {
                root: root_b,
                name: "Serena B must not rename Local B".into(),
            },
            WorkspaceImportCandidate {
                root: root_c.clone(),
                name: "Serena C".into(),
            },
            WorkspaceImportCandidate {
                root: root_d.clone(),
                name: "Serena D".into(),
            },
        ];

        assert_eq!(registry.import_serena(&candidates).unwrap(), 2);
        let after = registry.list();
        assert_eq!(after.registry_revision, 9);
        assert_eq!(after.workspaces[..2], [existing_a, existing_b]);
        assert_eq!(after.workspaces[2].name, "Serena C");
        assert_eq!(after.workspaces[2].root, root_c);
        assert_eq!(after.workspaces[2].generation, 1);
        assert_generated_workspace_id(&after.workspaces[2].id);
        assert_eq!(after.workspaces[3].name, "Serena D");
        assert_eq!(after.workspaces[3].root, root_d);
        assert_eq!(after.workspaces[3].generation, 1);
        assert_generated_workspace_id(&after.workspaces[3].id);
        assert_eq!(
            supervisor
                .desktop_selected_workspace()
                .map(|workspace| workspace.id),
            Some("local-b".into())
        );
        assert_eq!(
            WorkspaceRegistry::new(&SupervisorState::new(paths.clone()).unwrap()).list(),
            after
        );

        let bytes = fs::read(&paths.config_file).unwrap();
        assert_eq!(registry.import_serena(&candidates).unwrap(), 0);
        assert_eq!(registry.list(), after);
        assert_eq!(fs::read(paths.config_file).unwrap(), bytes);
    }

    #[test]
    fn serena_import_reuses_the_random_id_generator_after_remove() {
        let (_directory, paths, supervisor) = fixture(ManagerConfig::default());
        let root = paths.runtime_directory.parent().unwrap().join("imported");
        fs::create_dir(&root).unwrap();
        let candidate = WorkspaceImportCandidate {
            root: fs::canonicalize(root).unwrap(),
            name: "Imported".into(),
        };
        let registry = WorkspaceRegistry::new(&supervisor);

        assert_eq!(
            registry
                .import_serena(std::slice::from_ref(&candidate))
                .unwrap(),
            1
        );
        let first = registry.list().workspaces.pop().unwrap();
        assert_generated_workspace_id(&first.id);
        assert_eq!(registry.remove(&first.id).unwrap(), first);
        assert_eq!(registry.import_serena(&[candidate]).unwrap(), 1);
        let reimported = registry.list().workspaces.pop().unwrap();
        assert_generated_workspace_id(&reimported.id);
        assert_ne!(reimported.id, first.id);
        assert_eq!(reimported.generation, 1);
    }

    #[cfg(windows)]
    #[test]
    fn serena_import_persist_failure_keeps_memory_and_disk_unchanged() {
        let (_directory, paths, supervisor) = fixture(ManagerConfig {
            workspace_registry_revision: 13,
            ..ManagerConfig::default()
        });
        let root = paths.runtime_directory.parent().unwrap().join("imported");
        fs::create_dir(&root).unwrap();
        let registry = WorkspaceRegistry::new(&supervisor);
        let before = registry.list();
        let bytes = fs::read(&paths.config_file).unwrap();
        let original_permissions = fs::metadata(&paths.config_file).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&paths.config_file, readonly_permissions).unwrap();

        let result =
            WorkspaceRegistry::new(&supervisor).import_serena(&[WorkspaceImportCandidate {
                root: fs::canonicalize(root).unwrap(),
                name: "Imported".into(),
            }]);

        assert!(result.is_err());
        fs::set_permissions(&paths.config_file, original_permissions).unwrap();
        assert_eq!(WorkspaceRegistry::new(&supervisor).list(), before);
        assert_eq!(fs::read(paths.config_file).unwrap(), bytes);
    }

    #[test]
    fn register_uses_basename_and_rejects_empty_names_without_mutating() {
        let (_directory, paths, supervisor) = fixture(ManagerConfig::default());
        let root = paths
            .runtime_directory
            .parent()
            .unwrap()
            .join("basename-workspace");
        fs::create_dir(&root).unwrap();
        let registry = WorkspaceRegistry::new(&supervisor);

        assert_eq!(
            registry.register(root.clone(), None).unwrap().name,
            "basename-workspace"
        );
        let before = registry.list();
        let other_root = root.parent().unwrap().join("other-workspace");
        fs::create_dir(&other_root).unwrap();
        for name in [Some(String::new()), Some(" \t\n ".into())] {
            assert_eq!(
                registry.register(other_root.clone(), name),
                Err(WORKSPACE_NAME_INVALID.into())
            );
        }
        assert_eq!(registry.list(), before);
    }

    #[test]
    fn register_rejects_a_canonical_root_alias_without_changing_config() {
        let (_directory, paths, supervisor) = fixture(ManagerConfig::default());
        let root = paths
            .runtime_directory
            .parent()
            .unwrap()
            .join("duplicate-workspace");
        fs::create_dir(&root).unwrap();
        let alias = root
            .parent()
            .unwrap()
            .join(root.file_name().unwrap())
            .join("..")
            .join(root.file_name().unwrap());
        let registry = WorkspaceRegistry::new(&supervisor);
        registry.register(root, None).unwrap();
        let before = registry.list();
        let bytes = fs::read(&paths.config_file).unwrap();

        assert_eq!(
            registry.register(alias, Some("different name".into())),
            Err(WORKSPACE_ALREADY_EXISTS.into())
        );
        assert_eq!(registry.list(), before);
        assert_eq!(fs::read(paths.config_file).unwrap(), bytes);
    }

    #[cfg(windows)]
    #[test]
    fn register_rejects_windows_casing_and_separator_aliases() {
        use std::{
            ffi::OsString,
            os::windows::ffi::{OsStrExt, OsStringExt},
        };

        fn alias(path: &std::path::Path) -> PathBuf {
            PathBuf::from(OsString::from_wide(
                &path
                    .as_os_str()
                    .encode_wide()
                    .map(|unit| match unit {
                        value if value == u16::from(b'\\') => u16::from(b'/'),
                        value if (u16::from(b'a')..=u16::from(b'z')).contains(&value) => {
                            value - u16::from(b'a') + u16::from(b'A')
                        }
                        value => value,
                    })
                    .collect::<Vec<_>>(),
            ))
        }

        let (_directory, paths, supervisor) = fixture(ManagerConfig::default());
        let root = paths
            .runtime_directory
            .parent()
            .unwrap()
            .join("windows-alias");
        fs::create_dir(&root).unwrap();
        let registry = WorkspaceRegistry::new(&supervisor);
        registry.register(root.clone(), None).unwrap();
        let before = registry.list();

        assert_eq!(
            registry.register(alias(&fs::canonicalize(root).unwrap()), None),
            Err(WORKSPACE_ALREADY_EXISTS.into())
        );
        assert_eq!(registry.list(), before);
    }

    #[test]
    fn register_accepts_an_ordinary_directory_without_provider_or_filesystem_side_effects() {
        let (_directory, paths, supervisor) = fixture(ManagerConfig::default());
        let root = paths
            .runtime_directory
            .parent()
            .unwrap()
            .join("ordinary-workspace");
        fs::create_dir(&root).unwrap();

        let registered = WorkspaceRegistry::new(&supervisor)
            .register(root.clone(), None)
            .unwrap();

        assert_eq!(registered.root, fs::canonicalize(&root).unwrap());
        assert!(!root.join(".git").exists());
        assert!(!root.join(".serena").exists());
        assert!(!root.join(".codegraph").exists());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
        assert!(!paths.serena_home().exists());
    }

    #[cfg(windows)]
    #[test]
    fn register_persist_failure_keeps_memory_and_disk_registry_unchanged() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 13,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![workspace("project-8", "Existing", "C:/existing", 5)];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let root = paths
            .runtime_directory
            .parent()
            .unwrap()
            .join("persist-failure");
        fs::create_dir(&root).unwrap();
        let bytes = fs::read(&paths.config_file).unwrap();
        let original_permissions = fs::metadata(&paths.config_file).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&paths.config_file, readonly_permissions).unwrap();

        let result = WorkspaceRegistry::new(&supervisor).register(root, None);

        assert!(result.is_err());
        fs::set_permissions(&paths.config_file, original_permissions).unwrap();
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
    fn rename_persists_only_the_trimmed_name_and_one_new_revision() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 8,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/first", 3),
            workspace("second", "Second", "C:/second", 9),
        ];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let registry = WorkspaceRegistry::new(&supervisor);

        let renamed = registry
            .rename("second", "  Renamed second  ".into())
            .unwrap();
        let expected = WorkspaceRegistrySnapshot {
            registry_revision: 9,
            workspaces: vec![
                config.workspaces[0].clone(),
                workspace("second", "Renamed second", "C:/second", 9),
            ],
        };

        assert_eq!(renamed, expected.workspaces[1]);
        assert_eq!(registry.list(), expected);
        assert_eq!(
            WorkspaceRegistry::new(&SupervisorState::new(paths).unwrap()).list(),
            expected
        );
        let value = serde_json::to_value(renamed).unwrap();
        assert_eq!(value["id"], "second");
        assert_eq!(value["name"], "Renamed second");
        assert!(value["root"].is_string());
        assert_eq!(value["generation"], 9);
        assert!(value.get("workspace_registry_revision").is_none());
    }

    #[test]
    fn rename_allows_duplicate_names_and_rejects_invalid_or_unknown_ids_without_mutating() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 8,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/first", 3),
            workspace("second", "Second", "C:/second", 9),
        ];
        let (_directory, paths, supervisor) = fixture(config);
        let registry = WorkspaceRegistry::new(&supervisor);

        assert_eq!(
            registry.rename("second", " First ".into()).unwrap().name,
            "First"
        );
        let before = registry.list();
        let bytes = fs::read(&paths.config_file).unwrap();
        for name in [String::new(), " \t\n ".into()] {
            assert_eq!(
                registry.rename("second", name),
                Err(WORKSPACE_NAME_INVALID.into())
            );
        }
        assert_eq!(
            registry.rename("missing", "Other".into()),
            Err(WORKSPACE_NOT_FOUND.into())
        );
        assert_eq!(registry.list(), before);
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);

        assert_eq!(
            registry.rename("second", "  First  ".into()).unwrap().name,
            "First"
        );
        assert_eq!(registry.list(), before);
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);
    }

    #[test]
    fn reorder_persists_only_the_order_and_one_new_revision() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 8,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/first", 3),
            workspace("second", "Second", "C:/second", 9),
            workspace("third", "Third", "C:/third", 2),
        ];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let registry = WorkspaceRegistry::new(&supervisor);
        let expected = WorkspaceRegistrySnapshot {
            registry_revision: 9,
            workspaces: vec![
                config.workspaces[2].clone(),
                config.workspaces[0].clone(),
                config.workspaces[1].clone(),
            ],
        };

        let reordered = registry
            .reorder(vec!["third".into(), "first".into(), "second".into()])
            .unwrap();
        assert_eq!(reordered, expected);
        let value = serde_json::to_value(reordered).unwrap();
        assert_eq!(value["registryRevision"], 9);
        assert!(value.get("registry_revision").is_none());
        assert_eq!(value["workspaces"][0]["id"], "third");
        assert_eq!(
            WorkspaceRegistry::new(&SupervisorState::new(paths).unwrap()).list(),
            expected
        );
    }

    #[test]
    fn reorder_no_op_and_invalid_permutations_leave_registry_and_config_unchanged() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 8,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/first", 3),
            workspace("second", "Second", "C:/second", 9),
        ];
        let (_directory, paths, supervisor) = fixture(config);
        let registry = WorkspaceRegistry::new(&supervisor);
        let before = registry.list();
        let bytes = fs::read(&paths.config_file).unwrap();

        assert_eq!(
            registry
                .reorder(vec!["first".into(), "second".into()])
                .unwrap(),
            before
        );
        for ids in [
            vec![],
            vec!["first".into(), "first".into()],
            vec!["first".into(), "missing".into()],
            vec!["first".into(), "second".into(), "extra".into()],
        ] {
            assert!(registry.reorder(ids).is_err());
        }
        assert_eq!(registry.list(), before);
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);

        let (_directory, empty_paths, empty_supervisor) = fixture(ManagerConfig::default());
        let empty_registry = WorkspaceRegistry::new(&empty_supervisor);
        let empty_before = empty_registry.list();
        let empty_bytes = fs::read(&empty_paths.config_file).unwrap();
        assert_eq!(empty_registry.reorder(vec![]).unwrap(), empty_before);
        assert!(empty_registry.reorder(vec!["extra".into()]).is_err());
        assert_eq!(empty_registry.list(), empty_before);
        assert_eq!(fs::read(empty_paths.config_file).unwrap(), empty_bytes);
    }

    #[cfg(windows)]
    #[test]
    fn rename_and_reorder_persist_failures_keep_memory_and_disk_registry_unchanged() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 13,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/first", 3),
            workspace("second", "Second", "C:/second", 9),
        ];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let registry = WorkspaceRegistry::new(&supervisor);
        let bytes = fs::read(&paths.config_file).unwrap();

        let original_permissions = fs::metadata(&paths.config_file).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&paths.config_file, readonly_permissions).unwrap();
        let rename = registry.rename("second", "Renamed".into());
        assert!(rename.is_err());
        fs::set_permissions(&paths.config_file, original_permissions.clone()).unwrap();
        assert_eq!(registry.list().registry_revision, 13);
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);

        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&paths.config_file, readonly_permissions).unwrap();
        let reorder = registry.reorder(vec!["second".into(), "first".into()]);
        assert!(reorder.is_err());
        fs::set_permissions(&paths.config_file, original_permissions).unwrap();
        assert_eq!(registry.list().registry_revision, 13);
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);
        assert_eq!(
            SupervisorState::new(paths)
                .unwrap()
                .workspace_registry_config(),
            config
        );
    }

    #[test]
    fn remove_persists_only_the_target_entry_and_one_new_revision() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 8,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![
            workspace("first", "First", "C:/first", 3),
            workspace("second", "Second", "C:/second", 9),
            workspace("third", "Third", "C:/third", 2),
        ];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let registry = WorkspaceRegistry::new(&supervisor);

        let removed = registry.remove("second").unwrap();
        let expected = WorkspaceRegistrySnapshot {
            registry_revision: 9,
            workspaces: vec![config.workspaces[0].clone(), config.workspaces[2].clone()],
        };

        assert_eq!(removed, config.workspaces[1]);
        assert_eq!(registry.list(), expected);
        assert_eq!(
            WorkspaceRegistry::new(&SupervisorState::new(paths).unwrap()).list(),
            expected
        );
    }

    #[test]
    fn remove_missing_id_keeps_registry_revision_and_config_bytes_unchanged() {
        let mut config = ManagerConfig {
            workspace_registry_revision: 8,
            ..ManagerConfig::default()
        };
        config.workspaces = vec![workspace("only", "Only", "C:/only", 3)];
        let (_directory, paths, supervisor) = fixture(config.clone());
        let registry = WorkspaceRegistry::new(&supervisor);
        let bytes = fs::read(&paths.config_file).unwrap();

        assert_eq!(registry.remove("missing"), Err(WORKSPACE_NOT_FOUND.into()));
        assert_eq!(
            registry.list(),
            WorkspaceRegistrySnapshot {
                registry_revision: 8,
                workspaces: config.workspaces,
            }
        );
        assert_eq!(fs::read(paths.config_file).unwrap(), bytes);
    }

    #[test]
    fn remove_preserves_workspace_files_and_reregistering_uses_new_ids() {
        let (_directory, paths, supervisor) = fixture(ManagerConfig::default());
        let root = paths.runtime_directory.parent().unwrap().join("workspace");
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::create_dir_all(root.join(".serena")).unwrap();
        fs::create_dir_all(root.join(".codegraph")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        let files = [
            (root.join("marker.txt"), b"marker".as_slice()),
            (root.join("src/main.rs"), b"source".as_slice()),
            (root.join(".git/objects/keep"), b"git".as_slice()),
            (root.join(".serena/config.yml"), b"serena".as_slice()),
            (root.join(".codegraph/index.db"), b"index".as_slice()),
        ];
        for (path, contents) in &files {
            fs::write(path, contents).unwrap();
        }
        let registry = WorkspaceRegistry::new(&supervisor);
        let removed = registry
            .register(root.clone(), Some("Workspace".into()))
            .unwrap();

        assert_eq!(registry.remove(&removed.id).unwrap(), removed);
        assert!(root.is_dir());
        for (path, contents) in &files {
            assert_eq!(fs::read(path).unwrap(), *contents);
        }

        let reregistered = registry.register(root, None).unwrap();
        let other_root = paths
            .runtime_directory
            .parent()
            .unwrap()
            .join("other-workspace");
        fs::create_dir(&other_root).unwrap();
        let other = registry.register(other_root, None).unwrap();
        assert_ne!(reregistered.id, removed.id);
        assert_ne!(other.id, removed.id);
        assert_eq!(reregistered.generation, 1);
        assert_eq!(other.generation, 1);
    }

    #[cfg(windows)]
    #[test]
    fn remove_persist_failure_keeps_entry_config_and_workspace_files_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        fs::create_dir(&root).unwrap();
        let marker = root.join("marker.txt");
        fs::write(&marker, "preserve").unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        let config = ManagerConfig {
            workspace_registry_revision: 13,
            desktop_selected_workspace_id: Some("only".into()),
            workspaces: vec![workspace(
                "only",
                "Only",
                fs::canonicalize(&root).unwrap(),
                5,
            )],
            ..ManagerConfig::default()
        };
        config::save(&paths.config_file, &config).unwrap();
        let supervisor = SupervisorState::new(paths.clone()).unwrap();
        let bytes = fs::read(&paths.config_file).unwrap();
        let original_permissions = fs::metadata(&paths.config_file).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&paths.config_file, readonly_permissions).unwrap();

        let result = WorkspaceRegistry::new(&supervisor).remove("only");

        assert!(result.is_err());
        fs::set_permissions(&paths.config_file, original_permissions).unwrap();
        assert_eq!(
            WorkspaceRegistry::new(&supervisor).list().registry_revision,
            13
        );
        assert_eq!(fs::read(&paths.config_file).unwrap(), bytes);
        assert_eq!(fs::read(marker).unwrap(), b"preserve");
        assert_eq!(
            SupervisorState::new(paths)
                .unwrap()
                .workspace_registry_config(),
            config
        );
    }

    #[test]
    fn registration_result_serializes_with_the_tauri_workspace_shape() {
        let (_directory, paths, supervisor) = fixture(ManagerConfig::default());
        let root = paths
            .runtime_directory
            .parent()
            .unwrap()
            .join("serialized-workspace");
        fs::create_dir(&root).unwrap();

        let value = serde_json::to_value(
            WorkspaceRegistry::new(&supervisor)
                .register(root, Some("  Serialized  ".into()))
                .unwrap(),
        )
        .unwrap();

        assert_generated_workspace_id(value["id"].as_str().unwrap());
        assert_eq!(value["name"], "Serialized");
        assert!(value["root"].is_string());
        assert_eq!(value["generation"], 1);
        assert!(value.get("workspace_registry_revision").is_none());
    }
}

#![allow(
    dead_code,
    reason = "P2A2-001 provides the authority foundation before Request Authority callers migrate."
)]

use crate::{
    config::canonicalize_workspace_root, serena::SupervisorState,
    workspace_registry::WorkspaceRegistry,
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceLease {
    pub(crate) workspace_id: String,
    pub(crate) canonical_root: PathBuf,
    pub(crate) generation: u64,
}

pub(crate) struct WorkspaceResolver<'a> {
    supervisor: &'a SupervisorState,
}

impl<'a> WorkspaceResolver<'a> {
    pub(crate) fn new(supervisor: &'a SupervisorState) -> Self {
        Self { supervisor }
    }

    pub(crate) fn resolve(&self, id: &str) -> Result<WorkspaceLease, String> {
        let workspace = WorkspaceRegistry::new(self.supervisor).get(id)?;
        let canonical_root = canonicalize_workspace_root(&workspace.root).map_err(str::to_owned)?;

        Ok(WorkspaceLease {
            workspace_id: workspace.id,
            canonical_root,
            generation: workspace.generation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{self, AppPaths, ManagerConfig, Workspace},
        workspace_registry::{WORKSPACE_NOT_FOUND, WorkspaceRegistry},
    };
    use std::{fs, path::PathBuf};

    fn workspace(id: &str, name: &str, root: PathBuf, generation: u64) -> Workspace {
        Workspace {
            id: id.into(),
            name: name.into(),
            root,
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
    fn resolves_each_explicit_workspace_without_selection_or_filesystem_side_effects() {
        let directory = tempfile::tempdir().unwrap();
        let root_a = directory.path().join("ordinary-a");
        let root_b = directory.path().join("ordinary-b");
        fs::create_dir(&root_a).unwrap();
        fs::create_dir(&root_b).unwrap();
        let config = ManagerConfig {
            workspace_registry_revision: 7,
            desktop_selected_workspace_id: Some("b".into()),
            workspaces: vec![
                workspace("a", "A", root_a.clone(), 3),
                workspace("b", "B", root_b.clone(), 11),
            ],
            ..ManagerConfig::default()
        };
        let (_fixture_directory, paths, supervisor) = fixture(config.clone());
        let config_bytes = fs::read(&paths.config_file).unwrap();
        let resolver = WorkspaceResolver::new(&supervisor);

        let lease_a = resolver.resolve("a").unwrap();
        let lease_b = resolver.resolve("b").unwrap();

        assert_eq!(
            lease_a,
            WorkspaceLease {
                workspace_id: "a".into(),
                canonical_root: fs::canonicalize(&root_a).unwrap(),
                generation: 3,
            }
        );
        assert_eq!(
            lease_b,
            WorkspaceLease {
                workspace_id: "b".into(),
                canonical_root: fs::canonicalize(&root_b).unwrap(),
                generation: 11,
            }
        );
        assert_eq!(supervisor.workspace_registry_config(), config);
        assert_eq!(fs::read(&paths.config_file).unwrap(), config_bytes);
        assert!(!paths.serena_home().exists());
        for root in [&root_a, &root_b] {
            assert!(!root.join(".git").exists());
            assert!(!root.join(".serena").exists());
            assert!(!root.join(".codegraph").exists());
            assert!(fs::read_dir(root).unwrap().next().is_none());
        }
    }

    #[test]
    fn unknown_workspace_id_returns_the_stable_error() {
        let (_directory, _paths, supervisor) = fixture(ManagerConfig::default());

        assert_eq!(
            WorkspaceResolver::new(&supervisor).resolve("unknown"),
            Err(WORKSPACE_NOT_FOUND.into())
        );
    }

    #[test]
    fn deleted_registered_root_returns_not_found_without_removing_the_entry() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("deleted-root");
        fs::create_dir(&root).unwrap();
        let config = ManagerConfig {
            workspaces: vec![workspace("deleted", "Deleted", root.clone(), 5)],
            ..ManagerConfig::default()
        };
        let (_fixture_directory, _paths, supervisor) = fixture(config);
        fs::remove_dir(&root).unwrap();

        assert_eq!(
            WorkspaceResolver::new(&supervisor).resolve("deleted"),
            Err(crate::config::WORKSPACE_ROOT_NOT_FOUND.into())
        );
        assert_eq!(
            WorkspaceRegistry::new(&supervisor)
                .get("deleted")
                .unwrap()
                .root,
            root
        );
    }

    #[test]
    fn registered_root_replaced_by_file_returns_not_directory() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("replaced-root");
        fs::create_dir(&root).unwrap();
        let config = ManagerConfig {
            workspaces: vec![workspace("replaced", "Replaced", root.clone(), 5)],
            ..ManagerConfig::default()
        };
        let (_fixture_directory, _paths, supervisor) = fixture(config);
        fs::remove_dir(&root).unwrap();
        fs::write(&root, "ordinary file").unwrap();

        assert_eq!(
            WorkspaceResolver::new(&supervisor).resolve("replaced"),
            Err(crate::config::WORKSPACE_ROOT_NOT_DIRECTORY.into())
        );
    }

    #[test]
    fn rename_reorder_and_later_unrelated_changes_do_not_mutate_a_lease() {
        let directory = tempfile::tempdir().unwrap();
        let root_a = directory.path().join("a");
        let root_b = directory.path().join("b");
        fs::create_dir(&root_a).unwrap();
        fs::create_dir(&root_b).unwrap();
        let config = ManagerConfig {
            desktop_selected_workspace_id: Some("b".into()),
            workspaces: vec![
                workspace("a", "A", root_a.clone(), 17),
                workspace("b", "B", root_b, 23),
            ],
            ..ManagerConfig::default()
        };
        let (_fixture_directory, _paths, supervisor) = fixture(config);
        let resolver = WorkspaceResolver::new(&supervisor);
        let first_lease = resolver.resolve("a").unwrap();
        let registry = WorkspaceRegistry::new(&supervisor);

        registry.rename("a", "Renamed A".into()).unwrap();
        registry.reorder(vec!["b".into(), "a".into()]).unwrap();
        let second_lease = resolver.resolve("a").unwrap();
        registry.rename("b", "Renamed B".into()).unwrap();
        supervisor.select_desktop_workspace("a").unwrap();

        assert_eq!(first_lease, second_lease);
        assert_eq!(
            first_lease,
            WorkspaceLease {
                workspace_id: "a".into(),
                canonical_root: fs::canonicalize(root_a).unwrap(),
                generation: 17,
            }
        );
    }
}

use crate::{
    config::{canonicalize_workspace_root, same_workspace_root_identity},
    workspace_registry::WorkspaceImportCandidate,
};
use std::{fs, path::PathBuf};

pub(crate) struct ImportCandidates {
    pub(crate) candidates: Vec<WorkspaceImportCandidate>,
    pub(crate) warnings: Vec<String>,
    pub(crate) sources: Vec<PathBuf>,
}

// Read only registered roots and their configuration; never scan source trees or start LSPs.
pub(crate) fn read(sources: Vec<PathBuf>) -> Result<ImportCandidates, String> {
    let mut result = ImportCandidates {
        candidates: Vec::new(),
        warnings: Vec::new(),
        sources,
    };
    for source in &result.sources {
        let text = match fs::read_to_string(source) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("无法读取 {}：{e}", source.display())),
        };
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text)
            .map_err(|e| format!("无法解析 {}：{e}", source.display()))?;
        let paths = match &config["projects"] {
            serde_yaml_ng::Value::Null => continue,
            serde_yaml_ng::Value::Sequence(paths) => paths,
            _ => return Err(format!("{} 的 projects 必须是路径列表", source.display())),
        };
        for path in paths {
            let path = path
                .as_str()
                .ok_or_else(|| format!("{} 包含无效项目路径", source.display()))?;
            let loaded = (|| -> Result<(PathBuf, String), String> {
                let root = PathBuf::from(path);
                if !root.is_absolute() {
                    return Err("项目路径必须是绝对路径".into());
                }
                let root = canonicalize_workspace_root(&root).map_err(str::to_owned)?;
                let mut config: serde_yaml_ng::Value = serde_yaml_ng::from_str(
                    &fs::read_to_string(root.join(".serena/project.yml"))
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if !config.is_mapping() {
                    return Err("project.yml 必须是配置对象".into());
                }
                let local = root.join(".serena/project.local.yml");
                match fs::read_to_string(local) {
                    Ok(text) => {
                        let overrides: serde_yaml_ng::Value =
                            serde_yaml_ng::from_str(&text).map_err(|e| e.to_string())?;
                        // Serena creates a comment-only local override file by default.
                        if !overrides.is_null() {
                            let overrides = overrides
                                .as_mapping()
                                .ok_or("project.local.yml 必须是配置对象")?;
                            for (key, value) in overrides {
                                config[key.clone()] = value.clone();
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
                let name = config["project_name"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        root.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    });
                Ok((root, name))
            })();
            let (root, name) = match loaded {
                Ok(project) => project,
                Err(e) => {
                    result.warnings.push(format!("已跳过 {path}：{e}"));
                    continue;
                }
            };
            if result
                .candidates
                .iter()
                .any(|candidate| same_workspace_root_identity(&candidate.root, &root))
            {
                continue;
            }
            result
                .candidates
                .push(WorkspaceImportCandidate { name, root });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(base: &std::path::Path, name: &str) -> PathBuf {
        let root = base.join(name);
        fs::create_dir_all(root.join(".serena")).unwrap();
        fs::write(
            root.join(".serena/project.yml"),
            "project_name: shared\nlanguage: python\n",
        )
        .unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn read_collects_canonical_candidates_in_source_order_and_reads_local_names_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let one = project(dir.path(), "one");
        let two = project(dir.path(), "two");
        fs::write(one.join(".serena/project.local.yml"), "# local overrides\n").unwrap();
        fs::write(
            two.join(".serena/project.local.yml"),
            "project_name: override\n",
        )
        .unwrap();
        let sources: Vec<_> = ["user.yml", "managed.yml"]
            .iter()
            .map(|n| dir.path().join(n))
            .collect();
        fs::write(
            &sources[0],
            serde_json::json!({"projects": [one]}).to_string(),
        )
        .unwrap();
        fs::write(
            &sources[1],
            serde_json::json!({"projects": [one, two]}).to_string(),
        )
        .unwrap();
        let before = fs::read(&sources[0]).unwrap();
        let result = read(sources.clone()).unwrap();
        assert_eq!(result.candidates.len(), 2);
        assert_eq!(result.candidates[0].root, one);
        assert_eq!(result.candidates[0].name, "shared");
        assert_eq!(result.candidates[1].root, two);
        assert_eq!(result.candidates[1].name, "override");
        assert!(result.warnings.is_empty());
        assert_eq!(fs::read(&sources[0]).unwrap(), before);
        let again = read(sources).unwrap();
        assert_eq!(again.candidates, result.candidates);
    }

    #[test]
    fn read_skips_uninitialized_or_invalid_projects_and_does_not_discover_unregistered_roots() {
        let dir = tempfile::tempdir().unwrap();
        let valid = project(dir.path(), "valid");
        let bad = project(dir.path(), "bad");
        project(dir.path(), "unregistered");
        fs::write(bad.join(".serena/project.yml"), "[invalid").unwrap();
        let source = dir.path().join("registry.yml");
        fs::write(
            &source,
            serde_json::json!({"projects": [valid, bad, dir.path().join("missing")]}).to_string(),
        )
        .unwrap();
        let result = read(vec![source]).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].root, valid);
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn missing_registry_is_empty_but_malformed_registry_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("registry.yml");
        assert!(read(vec![source.clone()]).unwrap().candidates.is_empty());
        for text in ["[bad yaml", "projects: bad", "projects: [42]"] {
            fs::write(&source, text).unwrap();
            assert!(read(vec![source.clone()]).is_err());
        }
    }

    #[cfg(windows)]
    #[test]
    fn read_deduplicates_windows_casing_aliases_across_sources() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), "project");
        let casing_alias = PathBuf::from(root.to_string_lossy().to_ascii_uppercase());
        let first = dir.path().join("first.yml");
        let second = dir.path().join("second.yml");
        fs::write(&first, serde_json::json!({"projects": [root]}).to_string()).unwrap();
        fs::write(
            &second,
            serde_json::json!({"projects": [casing_alias]}).to_string(),
        )
        .unwrap();

        let result = read(vec![first, second]).unwrap();

        assert_eq!(result.candidates.len(), 1);
    }
}

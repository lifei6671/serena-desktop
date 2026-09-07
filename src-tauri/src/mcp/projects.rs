use crate::config::Workspace;
use serde::Serialize;
use std::{collections::HashSet, fs, path::PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub projects: Vec<Workspace>,
    pub warnings: Vec<String>,
    pub sources: Vec<PathBuf>,
}

// Read only registered roots and their configuration; never scan source trees or start LSPs.
pub fn read(sources: Vec<PathBuf>, previous: &[Workspace]) -> Result<SyncResult, String> {
    let mut result = SyncResult {
        projects: Vec::new(),
        warnings: Vec::new(),
        sources,
    };
    let mut seen = HashSet::new();
    let mut ids: HashSet<String> = previous.iter().map(|w| w.id.clone()).collect();
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
                let root = root.canonicalize().map_err(|e| e.to_string())?;
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
            if !seen.insert(root.clone()) {
                continue;
            }
            let id = if let Some(w) = previous.iter().find(|w| w.root == root) {
                w.id.clone()
            } else {
                let mut n = 1;
                while ids.contains(&format!("project-{n}")) {
                    n += 1;
                }
                let id = format!("project-{n}");
                ids.insert(id.clone());
                id
            };
            result.projects.push(Workspace { id, name, root });
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
    fn sync_merges_registries_preserves_ids_and_reads_local_names_without_writes() {
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
        let previous = vec![Workspace {
            id: "project-8".into(),
            name: "old".into(),
            root: one,
        }];
        let result = read(sources.clone(), &previous).unwrap();
        assert_eq!(result.projects.len(), 2);
        assert_eq!(result.projects[0].id, "project-8");
        assert_eq!(result.projects[0].name, "shared");
        assert_eq!(result.projects[1].name, "override");
        assert_ne!(result.projects[1].id, "project-8");
        assert!(result.warnings.is_empty());
        assert_eq!(fs::read(&sources[0]).unwrap(), before);
        let again = read(sources, &result.projects).unwrap();
        assert_eq!(again.projects, result.projects);
    }

    #[test]
    fn sync_skips_uninitialized_or_invalid_projects_and_does_not_discover_unregistered_roots() {
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
        let result = read(vec![source], &[]).unwrap();
        assert_eq!(result.projects.len(), 1);
        assert_eq!(result.projects[0].root, valid);
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn missing_registry_is_empty_but_malformed_registry_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("registry.yml");
        assert!(read(vec![source.clone()], &[]).unwrap().projects.is_empty());
        for text in ["[bad yaml", "projects: bad", "projects: [42]"] {
            fs::write(&source, text).unwrap();
            assert!(read(vec![source.clone()], &[]).is_err());
        }
    }
}

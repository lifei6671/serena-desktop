use super::process;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitArgs {
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub count: Option<u32>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}
fn git(root: &Path) -> Result<tokio::process::Command, String> {
    let exe = crate::serena::find_executable("git").ok_or("BACKEND_UNAVAILABLE: Git 未安装")?;
    let mut c = process::command(exe);
    c.arg("--no-pager")
        .arg("--no-optional-locks")
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LITERAL_PATHSPECS", "1");
    Ok(c)
}
pub async fn root(path: &Path, cancel: CancellationToken) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("INVALID_WORKSPACE: {e}"))?;
    let mut c = git(&canonical)?;
    c.args(["rev-parse", "--show-toplevel"]);
    let output = process::run(c, 8192, Duration::from_secs(10), cancel).await?;
    let root = PathBuf::from(output.text.trim())
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if root != canonical {
        return Err("INVALID_WORKSPACE: 请选择 Git 工作树根目录".into());
    }
    Ok(root)
}
pub async fn call(
    name: &str,
    root: &Path,
    args: GitArgs,
    cancel: CancellationToken,
) -> Result<process::Output, String> {
    let limit = args.max_bytes.unwrap_or(65536);
    if !(1..=262144).contains(&limit) {
        return Err("INVALID_PARAMS: max_bytes 必须为 1..262144".into());
    }
    let mut c = git(root)?;
    let reference = args.reference.as_deref().unwrap_or("HEAD");
    if reference.is_empty() || reference.starts_with('-') || reference.chars().any(char::is_control)
    {
        return Err("INVALID_PARAMS: reference 无效".into());
    }
    match name {
        "git_status" => {
            c.args([
                "status",
                "--porcelain=v1",
                "--branch",
                "--untracked-files=normal",
            ]);
        }
        "git_diff" => {
            c.args(["diff", "--no-ext-diff", "--no-textconv"]);
            match args.scope.as_deref().unwrap_or("unstaged") {
                "unstaged" => {}
                "staged" => {
                    c.arg("--cached");
                }
                "all" => {
                    c.arg("HEAD");
                }
                _ => return Err("INVALID_PARAMS: scope 必须是 unstaged/staged/all".into()),
            };
        }
        "git_log" => {
            let n = args.count.unwrap_or(20);
            if !(1..=100).contains(&n) {
                return Err("INVALID_PARAMS: count 必须为 1..100".into());
            }
            c.args([
                "log",
                "--no-decorate",
                "--format=%H %ad %s",
                "--date=iso-strict",
            ])
            .arg(format!("-{n}"))
            .arg(reference);
        }
        "git_show" => {
            c.args(["show", "--no-ext-diff", "--no-textconv", "--format=fuller"])
                .arg(reference);
        }
        "git_branch" => {
            c.args(["branch", "--list", "--no-color"]);
        }
        "git_worktree_list" => {
            c.args(["worktree", "list", "--porcelain"]);
        }
        _ => return Err("UNKNOWN_TOOL".into()),
    }
    if let Some(path) = args.path {
        if !matches!(name, "git_diff" | "git_log" | "git_show") {
            return Err("INVALID_PARAMS: 此工具不支持 path".into());
        }
        if Path::new(&path).is_absolute()
            || path.contains(':')
            || path.split(['/', '\\']).any(|p| p == "..")
        {
            return Err("INVALID_PATH".into());
        }
        c.arg("--").arg(path);
    }
    process::run(c, limit, Duration::from_secs(30), cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn worktree_and_readonly_commands() {
        let dir = tempfile::tempdir().unwrap();
        let mut init = process::command("git");
        init.arg("init").arg(dir.path());
        process::run(
            init,
            8192,
            Duration::from_secs(10),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let root = root(dir.path(), CancellationToken::new()).await.unwrap();
        std::fs::write(root.join("test.txt"), "hello").unwrap();
        let out = call(
            "git_status",
            &root,
            GitArgs::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(out.text.contains("test.txt"));
        assert_eq!(std::fs::read(root.join("test.txt")).unwrap(), b"hello");
        let bad = GitArgs {
            reference: Some("--output=owned".into()),
            ..Default::default()
        };
        assert!(
            call("git_show", &root, bad, CancellationToken::new())
                .await
                .is_err()
        );
        assert!(process::safe_relative(&root, "../outside").is_err());
        std::fs::write(root.join("binary.bin"), [255u8; 32]).unwrap();
        for args in [
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "fixture",
                "--no-gpg-sign",
            ],
        ] {
            let mut cmd = git(&root).unwrap();
            cmd.args(args);
            process::run(cmd, 8192, Duration::from_secs(10), CancellationToken::new())
                .await
                .unwrap();
        }
        let binary = call(
            "git_show",
            &root,
            GitArgs {
                reference: Some("HEAD:binary.bin".into()),
                max_bytes: Some(4),
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(binary.text.len() <= 4 && binary.truncated);
        let index_before = std::fs::read(root.join(".git/index")).unwrap();
        std::fs::write(root.join("test.txt"), "changed\n").unwrap();
        for name in super::super::registry::GITS {
            let result = call(name, &root, GitArgs::default(), CancellationToken::new())
                .await
                .unwrap();
            assert!(!result.truncated);
            assert!(!result.text.is_empty(), "{name}");
            assert!(
                call(
                    name,
                    &root,
                    GitArgs {
                        max_bytes: Some(0),
                        ..Default::default()
                    },
                    CancellationToken::new()
                )
                .await
                .is_err()
            );
            let small = call(
                name,
                &root,
                GitArgs {
                    max_bytes: Some(1),
                    ..Default::default()
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert!(small.truncated, "{name}");
        }
        assert_eq!(
            std::fs::read(root.join(".git/index")).unwrap(),
            index_before
        );
        assert_eq!(std::fs::read(root.join("test.txt")).unwrap(), b"changed\n");
        // A linked worktree has a .git file and must remain a valid root.
        let linked = dir.path().join("linked");
        let mut cmd = git(&root).unwrap();
        cmd.args(["worktree", "add", "--detach"]).arg(&linked);
        process::run(cmd, 8192, Duration::from_secs(10), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            super::root(&linked, CancellationToken::new())
                .await
                .unwrap(),
            linked.canonicalize().unwrap()
        );
    }
}

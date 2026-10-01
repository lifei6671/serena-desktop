//! CodeBuddy 原生 Apple Silicon discovery；共享 Codex 的 Mach-O 静态检查。

use std::{fs, path::PathBuf};

use super::discovery::{
    DiscoveryError, DiscoveryInput, DiscoveryProvenance, DiscoveryResult, DiscoverySource,
    MetadataStatus, ResolvedLaunchSpec, VersionMetadata,
};

/// 仅接受 canonical 绝对目录，排除空 PATH/current directory 和非目录项。
fn project_paths(input: &DiscoveryInput) -> Vec<(PathBuf, DiscoverySource)> {
    let mut entries = Vec::new();
    for (value, source) in [
        (&input.explicit_path, DiscoverySource::Explicit),
        (&input.process_path, DiscoverySource::Process),
    ] {
        if let Some(value) = value {
            entries.extend(std::env::split_paths(value).map(|path| (path, source)));
        }
    }
    entries.extend(
        input
            .safe_common_dirs
            .iter()
            .cloned()
            .map(|path| (path, DiscoverySource::SafeCommon)),
    );
    let mut projected = Vec::new();
    for (path, source) in entries {
        if !path.is_absolute() {
            continue;
        }
        let Ok(path) = fs::canonicalize(path) else {
            continue;
        };
        if path.is_dir() && !projected.iter().any(|(existing, _)| existing == &path) {
            projected.push((path, source));
        }
    }
    projected
}

/// 按 explicit、process PATH、安全目录顺序选取原生 CLI，不执行版本或 shell probe。
pub(crate) fn discover(input: DiscoveryInput) -> Result<DiscoveryResult, DiscoveryError> {
    if !cfg!(target_arch = "aarch64") {
        return Err(DiscoveryError::not_found(false));
    }
    let entries = project_paths(&input);
    let candidates = input
        .explicit_executable
        .into_iter()
        .map(|path| (path, DiscoverySource::Explicit))
        .chain(
            entries
                .iter()
                .map(|(path, source)| (path.join("codebuddy"), *source)),
        );
    for (candidate, source) in candidates {
        // 相对 explicit 路径同样不能引入当前工作目录作为执行来源。
        if !candidate.is_absolute() {
            continue;
        }
        let Ok(executable) = fs::canonicalize(candidate) else {
            continue;
        };
        if crate::agent::codex::macos_discovery::preflight(&executable).is_err() {
            continue;
        }
        return Ok(DiscoveryResult {
            launch_spec: ResolvedLaunchSpec {
                executable: executable.clone(),
                args: vec!["--acp".into()],
                path_projection: entries.iter().map(|(path, _)| path.clone()).collect(),
            },
            provenance: DiscoveryProvenance {
                source,
                resolved_executable: executable,
                wrapper_resolved: false,
                metadata_status: MetadataStatus::Missing,
            },
            metadata: VersionMetadata {
                product_version: None,
                base_version: None,
                package_version: None,
                status: MetadataStatus::Missing,
            },
        });
    }
    Err(DiscoveryError::not_found(
        entries
            .iter()
            .any(|(path, _)| path.join("buddycn").is_file()),
    ))
}

#[cfg(all(test, target_arch = "aarch64"))]
mod tests {
    use super::*;
    use std::{
        ffi::OsString,
        os::unix::fs::{PermissionsExt, symlink},
        path::Path,
    };

    /// 创建最小 Mach-O fixture；不运行文件或依赖本机 CodeBuddy 安装。
    fn native(path: &Path, cpu: u32, mode: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = Vec::from(0xfeedfacfu32.to_le_bytes());
        bytes.extend_from_slice(&cpu.to_le_bytes());
        bytes.resize(32, 0);
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// PATH 支持中文/空格/symlink，并只返回 canonical executable 与独立 ACP 参数。
    #[test]
    fn native_path_symlink_is_canonical_and_shell_free() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("真实 工具/codebuddy");
        native(&target, 0x0100000c, 0o755);
        let bin = root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        symlink(&target, bin.join("codebuddy")).unwrap();
        let result = discover(DiscoveryInput {
            process_path: Some(std::env::join_paths([&bin]).unwrap()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            result.launch_spec.executable,
            target.canonicalize().unwrap()
        );
        assert_eq!(result.launch_spec.args, [OsString::from("--acp")]);
        assert_eq!(result.provenance.source, DiscoverySource::Process);
        assert!(!result.provenance.wrapper_resolved);
    }

    /// Finder 空 PATH 通过安全目录找到 CLI；重复/相对/空目录不会进入子进程 PATH。
    #[test]
    fn safe_dirs_resolve_without_process_path_and_projection_deduplicates() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("home/.local/bin");
        native(&bin.join("codebuddy"), 0x0100000c, 0o755);
        let result = discover(DiscoveryInput {
            process_path: Some(OsString::from(":relative:")),
            safe_common_dirs: vec![bin.clone(), bin.clone(), PathBuf::from("relative")],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(result.provenance.source, DiscoverySource::SafeCommon);
        assert_eq!(
            result.launch_spec.path_projection,
            [bin.canonicalize().unwrap()]
        );
    }

    /// 无 execute bit、x86、脚本和目录都不可作为原生 CLI；失败继续遍历后续候选。
    #[test]
    fn invalid_candidates_do_not_block_later_native_binary() {
        let root = tempfile::tempdir().unwrap();
        let bins: Vec<_> = (0..5).map(|n| root.path().join(n.to_string())).collect();
        native(&bins[0].join("codebuddy"), 0x0100000c, 0o644);
        native(&bins[1].join("codebuddy"), 0x01000007, 0o755);
        native(&bins[2].join("codebuddy"), 0x0100000c, 0o755);
        fs::write(bins[2].join("codebuddy"), b"#!/bin/sh\n").unwrap();
        fs::create_dir_all(bins[3].join("codebuddy")).unwrap();
        for bin in &bins[..4] {
            assert!(
                discover(DiscoveryInput {
                    safe_common_dirs: vec![bin.clone()],
                    ..Default::default()
                })
                .is_err()
            );
        }
        native(&bins[4].join("codebuddy"), 0x0100000c, 0o755);
        let result = discover(DiscoveryInput {
            safe_common_dirs: bins.clone(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            result.launch_spec.executable,
            bins[4].join("codebuddy").canonicalize().unwrap()
        );
    }
}

//! 本地确定性体验策略；不是 OS sandbox，也不执行命令或调用模型。
use crate::{workspace_path::WorkspacePathResolver, workspace_resolver::WorkspaceLease};
use agent_client_protocol::schema::v1::{
    RequestPermissionRequest, ToolCallContent, ToolCallUpdateFields, ToolKind,
};
use serde_json::Value;
use std::path::Path;

/// 冻结执行记录的权限身份，不读取当前 active workspace。
pub(crate) struct Authority {
    pub(crate) lease: WorkspaceLease,
    pub(crate) mode: String,
}

/// 策略仅产生单次决定；advertised option 的选择由 dispatcher 执行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    AutoAllowOnce,
    RejectOnce,
}

/// 对 exact tool 合并后的结构化字段执行本地判断。
pub(crate) fn evaluate(
    authority: &Authority,
    tool: &ToolCallUpdateFields,
    _request: &RequestPermissionRequest,
) -> Decision {
    if !matches!(authority.mode.as_str(), "read_only" | "workspace_write") {
        return Decision::RejectOnce;
    }
    let allowed = match tool.kind {
        Some(ToolKind::Fetch) => true,
        Some(ToolKind::Read | ToolKind::Search) => paths_valid(authority, tool),
        Some(ToolKind::Edit | ToolKind::Delete | ToolKind::Move) => {
            authority.mode == "workspace_write" && paths_valid(authority, tool)
        }
        Some(ToolKind::Execute) => command_valid(authority, tool),
        _ => false,
    };
    if allowed {
        Decision::AutoAllowOnce
    } else {
        Decision::RejectOnce
    }
}

/// 收集所有明确路径字段；坏类型也拒绝，避免仅检查一个目的路径。
fn input_paths<'a>(value: &'a Value, paths: &mut Vec<&'a str>) -> bool {
    match value {
        Value::Object(map) => map.iter().all(|(key, value)| {
            if matches!(
                key.as_str(),
                "path"
                    | "file_path"
                    | "filePath"
                    | "source"
                    | "destination"
                    | "from"
                    | "to"
                    | "old_path"
                    | "new_path"
                    | "oldPath"
                    | "newPath"
            ) {
                if let Some(path) = value.as_str() {
                    paths.push(path);
                    true
                } else {
                    false
                }
            } else {
                input_paths(value, paths)
            }
        }),
        Value::Array(items) => items.iter().all(|item| input_paths(item, paths)),
        _ => true,
    }
}

/// locations、diff 与 raw input 中出现的目标必须全部可验证。
fn paths_valid(authority: &Authority, tool: &ToolCallUpdateFields) -> bool {
    let mut paths = Vec::new();
    if let Some(locations) = &tool.locations {
        for location in locations {
            if !location.path.is_absolute() {
                return false;
            }
            let Some(path) = location.path.to_str() else {
                return false;
            };
            paths.push(path);
        }
    }
    if let Some(content) = &tool.content {
        for item in content {
            if let ToolCallContent::Diff(diff) = item {
                let Some(path) = diff.path.to_str() else {
                    return false;
                };
                paths.push(path);
            }
        }
    }
    if let Some(input) = &tool.raw_input {
        if !input_paths(input, &mut paths) {
            return false;
        }
    }
    !paths.is_empty()
        && paths
            .into_iter()
            .all(|path| workspace_path(authority, path))
}

/// 用既有 canonical/nearest-existing resolver 验证路径，禁止父级跳转。
fn workspace_path(authority: &Authority, value: &str) -> bool {
    let resolver = WorkspacePathResolver::new(&authority.lease);
    if value == "." {
        return resolver.root().is_ok();
    }
    if Path::new(value).is_absolute() {
        resolver.resolve_absolute(Path::new(value)).is_ok()
    } else {
        resolver.resolve(value).is_ok()
    }
}

/// 临时目录仅用于显式编译输出；不会扩大文件工具可写范围。
fn output_path(authority: &Authority, value: &str) -> bool {
    if workspace_path(authority, value) {
        return true;
    }
    let Ok(root) = std::fs::canonicalize(std::env::temp_dir()) else {
        return false;
    };
    [Some(root), system_temp_alias()]
        .into_iter()
        .flatten()
        .any(|root| {
            let lease = WorkspaceLease {
                workspace_id: authority.lease.workspace_id.clone(),
                canonical_root: root,
                generation: authority.lease.generation,
            };
            Path::new(value).is_absolute()
                && WorkspacePathResolver::new(&lease)
                    .resolve_absolute(Path::new(value))
                    .is_ok()
        })
}

/// Unix 系统临时目录使用原始 alias，由 resolver 做最终 canonical 检查。
fn system_temp_alias() -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    {
        std::fs::canonicalize("/tmp").ok()
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// 简单引号分词，不解释 shell expansion；所有 shell 控制字符先整体拒绝。
fn words(command: &str) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for c in command.chars() {
        if c == '\'' || c == '"' {
            if quote == Some(c) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(c);
            } else {
                current.push(c);
            }
        } else if c.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if quote.is_some() {
        return None;
    }
    if !current.is_empty() {
        result.push(current);
    }
    Some(result)
}

/// 检查有限的单条开发命令，最多容许一个工作区 cd 前缀。
fn command_valid(authority: &Authority, tool: &ToolCallUpdateFields) -> bool {
    let Some(command) = tool
        .raw_input
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|v| v.get("command"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    if command.trim().is_empty()
        || command.contains([
            ';', '|', '>', '<', '`', '\n', '\r', '$', '^', '%', '!', '(', ')', '*', '?', '{', '}',
            '[', ']', '~',
        ])
    {
        return false;
    }
    #[cfg(not(windows))]
    if command.contains('\\') {
        return false;
    }
    let parts: Vec<_> = command.split("&&").collect();
    let mut cwd = authority.lease.canonical_root.clone();
    let command = match parts.as_slice() {
        [single] => *single,
        [prefix, command] => {
            let Some(prefix) = words(prefix) else {
                return false;
            };
            if prefix.len() != 2 || prefix[0] != "cd" || !workspace_path(authority, &prefix[1]) {
                return false;
            }
            let resolved = if Path::new(&prefix[1]).is_absolute() {
                WorkspacePathResolver::new(&authority.lease).resolve_absolute(Path::new(&prefix[1]))
            } else if prefix[1] == "." {
                WorkspacePathResolver::new(&authority.lease).root()
            } else {
                WorkspacePathResolver::new(&authority.lease).resolve(&prefix[1])
            };
            let Ok(resolved) = resolved else {
                return false;
            };
            cwd = resolved;
            if !cwd.is_dir() {
                return false;
            }
            *command
        }
        _ => return false,
    };
    if command.contains('&') {
        return false;
    }
    let Some(tokens) = words(command) else {
        return false;
    };
    let Some(program) = tokens.first() else {
        return false;
    };
    // 只接受程序名；绝对可执行路径和环境赋值不能伪装 allowlist。
    if program.contains('/') || program.contains('=') {
        return false;
    }
    let args = &tokens[1..];
    let sub = args.first().map(String::as_str).unwrap_or("");
    let readonly = matches!(
        program.as_str(),
        "pwd" | "ls" | "cat" | "head" | "tail" | "grep" | "rg" | "find"
    );
    let write = authority.mode == "workspace_write";
    let allowed = if readonly {
        !args.iter().any(|v| {
            matches!(
                v.as_str(),
                "-exec"
                    | "-execdir"
                    | "-delete"
                    | "-ok"
                    | "-okdir"
                    | "-fprint"
                    | "-fprint0"
                    | "-fprintf"
                    | "-fls"
            )
        })
    } else {
        match program.as_str() {
            "git" => matches!(sub, "status" | "diff" | "log" | "show") || write && sub == "fetch",
            // 直接 rustc 仅批准普通编译/测试，不开放 codegen 或 nightly 高级参数。
            "rustc" => {
                write
                    && !args.iter().any(|arg| {
                        arg.starts_with("-C")
                            || arg.starts_with("-Z")
                            || arg == "--codegen"
                            || arg.starts_with("--codegen=")
                    })
            }
            "cargo" => {
                write
                    && matches!(
                        sub,
                        "check"
                            | "test"
                            | "build"
                            | "fmt"
                            | "clippy"
                            | "doc"
                            | "metadata"
                            | "fetch"
                    )
            }
            "npm" | "pnpm" | "yarn" => {
                write
                    && matches!(
                        sub,
                        "test" | "build" | "lint" | "typecheck" | "check" | "ci" | "install"
                    )
                    // 安装不能显式切换到全局；同时覆盖等号、分离值和 npm 短参数组合。
                    && !(matches!(sub, "install" | "ci")
                        && args.iter().enumerate().any(|(index, arg)| {
                            let (flag, value) = arg
                                .split_once('=')
                                .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
                            matches!(flag, "-g" | "--global")
                                || matches!(flag, "--location" | "-L")
                                    && value
                                        .or_else(|| args.get(index + 1).map(String::as_str))
                                        == Some("global")
                                || program == "npm"
                                    && (matches!(flag, "--g" | "-global")
                                        || flag.strip_prefix('-').is_some_and(|shorts| {
                                            shorts.contains('g')
                                                && shorts.chars().all(|c| {
                                                    "acfgLdsqlmnpCSBDEOPhHvwy".contains(c)
                                                })
                                        }))
                        }))
            }
            "go" => {
                write
                    && (matches!(sub, "test" | "build" | "vet" | "fmt")
                        || sub == "mod" && args.get(1).map(String::as_str) == Some("download"))
                    // Go 可替换测试/工具链执行器；单/双横线、分离值和等号形式均拒绝。
                    && !args.iter().any(|arg| {
                        matches!(
                            arg.split_once('=').map_or(arg.as_str(), |(flag, _)| flag),
                            "-exec" | "--exec" | "-toolexec" | "--toolexec" | "-vettool" | "--vettool"
                        )
                    })
            }
            "pytest" => write,
            "python" | "python3" => {
                write
                    && args.first().map(String::as_str) == Some("-m")
                    && args.get(1).map(String::as_str) == Some("pytest")
            }
            _ => false,
        }
    };
    if !allowed || args.iter().any(|arg| arg.starts_with('@')) {
        return false;
    }
    // 命令选项中的可执行注入、配置绕行和 git 外部 helper 不自动批准。
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--pre"
                | "--hostname-bin"
                | "--exec"
                | "--config"
                | "--config-env"
                | "--ext-diff"
                | "--textconv"
                | "--upload-pack"
                | "--receive-pack"
                | "-c"
        ) || arg.starts_with("--pre=")
            || arg.starts_with("--hostname-bin=")
            || arg.starts_with("--config=")
            || arg.starts_with("--upload-pack=")
    }) {
        return false;
    }
    // 相对参数以实际 cd 目录解释，绝对参数仍由冻结 workspace 根验证。
    let command_path = |value: &str| -> Option<String> {
        let path = Path::new(value);
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return None;
        }
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        path.to_str().map(str::to_owned)
    };
    let mut output_next = false;
    for arg in args {
        if output_next {
            if !command_path(arg).is_some_and(|p| output_path(authority, &p)) {
                return false;
            }
            output_next = false;
            continue;
        }
        if matches!(program.as_str(), "rustc" | "cargo")
            && matches!(arg.as_str(), "-o" | "--out-dir" | "--target-dir")
        {
            output_next = true;
            continue;
        }
        if let Some(path) = arg
            .strip_prefix("--out-dir=")
            .or_else(|| arg.strip_prefix("--target-dir="))
        {
            if !matches!(program.as_str(), "rustc" | "cargo")
                || !command_path(path).is_some_and(|p| output_path(authority, &p))
            {
                return false;
            }
            continue;
        }
        if program == "rustc" && arg.starts_with("-o") && arg != "-o" {
            return false;
        }
        if arg.starts_with("--output") || arg.starts_with("--emit=") {
            return false;
        }
        // 拼接路径选项不能按普通相对文件名检查。
        if arg.starts_with('-') && !arg.contains('=') && (arg.contains('/') || arg.contains('\\')) {
            return false;
        }
        let value = arg.split_once('=').map_or(arg.as_str(), |(_, value)| value);
        if Path::new(value).is_absolute()
            || value.contains('/')
            || value.contains('\\')
            || value == ".."
            || !value.starts_with('-')
        {
            if !command_path(value).is_some_and(|p| workspace_path(authority, &p))
                && !(readonly && readonly_system_path(value))
            {
                return false;
            }
        }
    }
    !output_next
}

/// 只读工具的有限系统/toolchain 根目录例外，同样检查 canonical escape。
fn readonly_system_path(value: &str) -> bool {
    if !Path::new(value).is_absolute() {
        return false;
    }
    [
        "/usr",
        "/bin",
        "/sbin",
        "/System/Library",
        "/Library/Developer",
    ]
    .iter()
    .any(|root| {
        let Ok(root) = std::fs::canonicalize(root) else {
            return false;
        };
        let lease = WorkspaceLease {
            workspace_id: "readonly-system".into(),
            canonical_root: root,
            generation: 0,
        };
        WorkspacePathResolver::new(&lease)
            .resolve_absolute(Path::new(value))
            .is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 为每项测试生成独立冻结 workspace。
    fn authority(root: &Path, mode: &str) -> Authority {
        Authority {
            lease: WorkspaceLease {
                workspace_id: "fixture".into(),
                canonical_root: std::fs::canonicalize(root).unwrap(),
                generation: 9,
            },
            mode: mode.into(),
        }
    }

    /// 测试与生产一致地使用 typed ACP 请求。
    fn decision(authority: &Authority, kind: &str, input: Value) -> Decision {
        let request: RequestPermissionRequest = serde_json::from_value(json!({"sessionId":"session", "toolCall":{"toolCallId":"tool","kind":kind,"rawInput":input},"options":[]})).unwrap();
        evaluate(authority, &request.tool_call.fields, &request)
    }

    #[test]
    fn file_policy_checks_mode_all_paths_and_missing_targets() {
        let root = tempfile::tempdir().unwrap();
        let auth = authority(root.path(), "workspace_write");
        assert_eq!(
            decision(
                &auth,
                "edit",
                json!({"file_path":root.path().join("new/file.rs")})
            ),
            Decision::AutoAllowOnce
        );
        assert_eq!(
            decision(
                &auth,
                "edit",
                json!({"file_path":root.path().join("../outside")})
            ),
            Decision::RejectOnce
        );
        assert_eq!(
            decision(
                &auth,
                "move",
                json!({"source":"inside", "destination":"/outside"})
            ),
            Decision::RejectOnce
        );
        assert_eq!(decision(&auth, "read", json!({})), Decision::RejectOnce);
        assert_eq!(
            decision(&auth, "read", json!({"path":"file.rs"})),
            Decision::AutoAllowOnce
        );
        assert_eq!(
            decision(
                &authority(root.path(), "read_only"),
                "edit",
                json!({"path":"file.rs"})
            ),
            Decision::RejectOnce
        );
        for mode in ["read_only", "workspace_write"] {
            assert_eq!(
                decision(&authority(root.path(), mode), "fetch", json!({})),
                Decision::AutoAllowOnce
            );
        }
    }

    #[test]
    fn development_command_matrix_and_real_rustc_case() {
        let root = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let auth = authority(root.path(), "workspace_write");
        for command in [
            "cargo test",
            "cargo check",
            "npm test",
            "pnpm install",
            "yarn build",
            "go test ./...",
            "go mod download",
            "pytest",
            "python -m pytest",
            "git status",
            "git fetch",
            "cd . && cargo test",
        ] {
            assert_eq!(
                decision(&auth, "execute", json!({"command":command})),
                Decision::AutoAllowOnce,
                "{command}"
            );
        }
        let command = format!(
            "rustc --test '{}' -o '{}'",
            root.path().join("test.rs").display(),
            std::fs::canonicalize(temp.path())
                .unwrap()
                .join("test-bin")
                .display()
        );
        assert_eq!(
            decision(&auth, "execute", json!({"command":command})),
            Decision::AutoAllowOnce
        );
        for command in [
            "git push",
            "git reset --hard",
            "git clean",
            "git checkout main",
            "sudo cargo test",
            "bash -c 'cargo test'",
            "cargo test | cat",
            "cargo test; pwd",
            "cargo test > result",
            "cargo test $(pwd)",
            "cargo test &",
            "npm publish",
            "curl https://example.com",
            "find . -delete",
            "git diff --ext-diff",
            "cargo test\npwd",
        ] {
            assert_eq!(
                decision(&auth, "execute", json!({"command":command})),
                Decision::RejectOnce,
                "{command}"
            );
        }
        let read = authority(root.path(), "read_only");
        assert_eq!(
            decision(&read, "execute", json!({"command":"cargo test"})),
            Decision::RejectOnce
        );
        assert_eq!(
            decision(&read, "execute", json!({"command":"git status"})),
            Decision::AutoAllowOnce
        );
        assert_eq!(
            decision(&read, "execute", json!({"command":"git fetch"})),
            Decision::RejectOnce
        );
        assert_eq!(
            decision(
                &auth,
                "execute",
                json!({"command":"rustc file.rs -o /outside"})
            ),
            Decision::RejectOnce
        );
        assert_eq!(
            decision(
                &auth,
                "execute",
                json!({"command":"rustc file.rs -o/outside"})
            ),
            Decision::RejectOnce
        );
        assert_eq!(
            decision(
                &auth,
                "execute",
                json!({"command":"rustc file.rs --emit=link=/outside"})
            ),
            Decision::RejectOnce
        );
        assert_eq!(
            decision(&authority(root.path(), "unknown"), "fetch", json!({})),
            Decision::RejectOnce
        );
    }

    /// 全局安装及等价短参数拒绝，普通 workspace 安装/ci 仍自动批准。
    #[test]
    fn package_install_rejects_explicit_global_flags() {
        let root = tempfile::tempdir().unwrap();
        let auth = authority(root.path(), "workspace_write");
        for command in [
            "npm install -g foo",
            "npm install --global foo",
            "pnpm install -g foo",
            "npm install --location=global foo",
            "npm install --location global foo",
            "npm install -L global foo",
            "npm install -L=global foo",
            "npm install --global=true foo",
            "npm install -g=true foo",
            "npm install -global foo",
            "npm install --g foo",
            "npm install -sg foo",
            "npm install -gpld foo",
            "npm ci --global",
            "pnpm install --global foo",
            "yarn install -g foo",
            "yarn install --global foo",
        ] {
            assert_eq!(
                decision(&auth, "execute", json!({"command":command})),
                Decision::RejectOnce,
                "{command}"
            );
        }
        for command in [
            "npm install",
            "pnpm install",
            "npm ci",
            "yarn install",
            "npm install -D foo",
            "npm install -sp foo",
            "npm install --location=project",
            "npm install -L project",
        ] {
            assert_eq!(
                decision(&auth, "execute", json!({"command":command})),
                Decision::AutoAllowOnce,
                "{command}"
            );
        }
    }

    /// rustc 高级参数及拼接形式拒绝，真实 workspace 测试源到 OS temp 输出仍允许。
    #[test]
    fn rustc_rejects_codegen_and_unstable_options() {
        let root = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let auth = authority(root.path(), "workspace_write");
        let source = auth.lease.canonical_root.join("file.rs");
        std::fs::write(&source, "#[test] fn passes() {}\n").unwrap();
        for command in [
            "rustc file.rs -C linker=sh",
            "rustc file.rs -Clinker=sh",
            "rustc file.rs -C=linker=sh",
            "rustc file.rs -Z unstable-options",
            "rustc file.rs -Zunstable-options",
            "rustc file.rs -Z=unstable-options",
            "rustc file.rs --codegen linker=sh",
            "rustc file.rs --codegen=linker=sh",
        ] {
            assert_eq!(
                decision(&auth, "execute", json!({"command":command})),
                Decision::RejectOnce,
                "{command}"
            );
        }
        for command in [
            "rustc file.rs".to_owned(),
            "rustc file.rs -g -O --edition=2024".to_owned(),
            format!(
                "rustc --test '{}' -o '{}'",
                source.display(),
                std::fs::canonicalize(temp.path())
                    .unwrap()
                    .join("test-bin")
                    .display()
            ),
        ] {
            assert_eq!(
                decision(&auth, "execute", json!({"command":command})),
                Decision::AutoAllowOnce,
                "{command}"
            );
        }
    }

    /// 同一 allowlist 的 Go 外部执行器单横线选项不自动批准。
    #[test]
    fn go_rejects_external_executor_options() {
        let root = tempfile::tempdir().unwrap();
        let auth = authority(root.path(), "workspace_write");
        for command in [
            "go test -exec sh ./...",
            "go test -exec=sh ./...",
            "go build -toolexec sh ./...",
            "go test -toolexec=sh ./...",
            "go vet -vettool sh ./...",
            "go vet -vettool=sh ./...",
            "go test --exec sh ./...",
            "go test --exec=sh ./...",
            "go build --toolexec sh ./...",
            "go test --toolexec=sh ./...",
            "go vet --vettool sh ./...",
            "go vet --vettool=sh ./...",
        ] {
            assert_eq!(
                decision(&auth, "execute", json!({"command":command})),
                Decision::RejectOnce,
                "{command}"
            );
        }
        for command in ["go test ./...", "go build ./...", "go vet ./..."] {
            assert_eq!(
                decision(&auth, "execute", json!({"command":command})),
                Decision::AutoAllowOnce,
                "{command}"
            );
        }
    }

    /// 任意位置或 Diff 逃逸均拒绝；请求缺少路径、坏 command 或未知类型不猜测。
    #[test]
    fn structured_targets_and_command_boundaries() {
        let root = tempfile::tempdir().unwrap();
        let auth = authority(root.path(), "workspace_write");
        let inside = auth.lease.canonical_root.join("file.rs");
        let outside = auth.lease.canonical_root.parent().unwrap().join("outside");
        for field in [
            "path",
            "file_path",
            "source",
            "destination",
            "from",
            "to",
            "old_path",
            "new_path",
        ] {
            let input = json!({"file_path":inside, "nested": {field: outside}});
            assert_eq!(
                decision(&auth, "edit", input),
                Decision::RejectOnce,
                "{field}"
            );
        }
        for update in [
            json!({"kind":"edit", "locations":[{"path":inside},{"path":outside}]}),
            json!({"kind":"edit", "rawInput":{"file_path":inside}, "content":[{"type":"diff","path":outside,"oldText":null,"newText":"x"}]}),
            json!({"kind":"edit", "locations":[{"path":"relative"}]}),
        ] {
            let fields: ToolCallUpdateFields = serde_json::from_value(update).unwrap();
            let request: RequestPermissionRequest = serde_json::from_value(
                json!({"sessionId":"s","toolCall":{"toolCallId":"t"},"options":[]}),
            )
            .unwrap();
            assert_eq!(evaluate(&auth, &fields, &request), Decision::RejectOnce);
        }
        for kind in ["think", "switch_mode", "other"] {
            assert_eq!(
                decision(&auth, kind, json!({"path":inside})),
                Decision::RejectOnce
            );
        }
        for input in [
            json!("cargo test"),
            json!({}),
            json!({"command":1}),
            json!({"command":""}),
        ] {
            assert_eq!(decision(&auth, "execute", input), Decision::RejectOnce);
        }
        for command in [
            "cargo test && cargo build",
            "sh -c 'cargo test'",
            "cmd /c cargo test",
            "pwsh -Command cargo test",
            "powershell -Command cargo test",
            "ssh host",
            "scp file host",
            "rsync file host",
            "deploy",
            "release",
            "publish",
            "push",
            "rustc @args",
            "find . -fprintf output",
            "rg --pre=evil pattern",
            "cargo test\rpwd",
        ] {
            assert_eq!(
                decision(&auth, "execute", json!({"command":command})),
                Decision::RejectOnce,
                "{command}"
            );
        }
        std::fs::create_dir(auth.lease.canonical_root.join("sub")).unwrap();
        assert_eq!(
            decision(
                &auth,
                "execute",
                json!({"command":format!("cd sub && rustc --test input.rs -o '{}'",inside.display())})
            ),
            Decision::AutoAllowOnce
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            decision(
                &auth,
                "execute",
                json!({"command":"rustc --test file.rs -o /tmp/codebuddy-policy-test-bin"})
            ),
            Decision::AutoAllowOnce
        );
        #[cfg(windows)]
        assert_eq!(
            decision(
                &auth,
                "execute",
                json!({"command":format!("rustc --test '{}' -o '{}'",inside.display(),std::env::temp_dir().join("test-bin").display())})
            ),
            Decision::AutoAllowOnce
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_parent_and_missing_target_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        std::fs::create_dir(root.path().join("sub")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("sub/link")).unwrap();
        let auth = authority(root.path(), "workspace_write");
        assert_eq!(
            decision(&auth, "execute", json!({"command":"cd sub && cat link"})),
            Decision::RejectOnce
        );
        for path in [
            root.path().join("link"),
            root.path().join("link/new/file.rs"),
        ] {
            assert_eq!(
                decision(&auth, "edit", json!({"path":path})),
                Decision::RejectOnce
            );
        }
    }
}

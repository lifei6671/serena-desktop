use crate::{logs, serena};
use std::{env, path::PathBuf, time::Duration};

pub fn install_serena(app_log: &std::path::Path) -> Result<(), String> {
    let uv = match find_uv() {
        Some(path) => path,
        None => {
            logs::append(app_log, "installer", "未发现 uv，准备通过 winget 安装");
            install_uv(app_log)?;
            find_uv().ok_or_else(|| {
                "winget 已完成，但仍未找到 uv。请重新登录 Windows 或按官方说明手工安装 uv。"
                    .to_string()
            })?
        }
    };

    logs::append(app_log, "installer", "开始安装 Serena");
    let mut command = serena::hidden_command(&uv);
    command.args(["tool", "install", "-p", "3.13", "serena-agent"]);
    let output = serena::run_with_timeout(command, Duration::from_secs(15 * 60), "安装 Serena")?;
    ensure_success("安装 Serena", output, app_log)
}

fn install_uv(app_log: &std::path::Path) -> Result<(), String> {
    let winget = serena::find_executable("winget").ok_or_else(|| {
        "未发现 uv，也未发现 winget。请按 Serena 官方安装说明先安装 uv。".to_string()
    })?;
    let mut command = serena::hidden_command(winget);
    command.args([
        "install",
        "--id",
        "astral-sh.uv",
        "-e",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
    ]);
    let output = serena::run_with_timeout(command, Duration::from_secs(10 * 60), "安装 uv")?;
    ensure_success("安装 uv", output, app_log)
}

fn find_uv() -> Option<PathBuf> {
    serena::find_executable("uv")
        .or_else(|| serena::user_local_candidate("uv"))
        .or_else(|| {
            env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .map(|root| {
                    root.join("Microsoft")
                        .join("WinGet")
                        .join("Links")
                        .join("uv.exe")
                })
                .filter(|path| path.is_file())
        })
}

fn ensure_success(
    action: &str,
    output: serena::CapturedOutput,
    app_log: &std::path::Path,
) -> Result<(), String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        logs::append(app_log, "installer", &format!("{action}完成"));
        return Ok(());
    }

    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    let detail = truncate(detail, 8_000);
    let message = format!("{action}失败（{}）：{}", output.status, detail);
    logs::append(app_log, "installer", &message);
    Err(message)
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut result = value.chars().take(max_chars).collect::<String>();
    result.push_str("\n…输出已截断，请查看应用日志。");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_preserves_short_messages() {
        assert_eq!(truncate("error", 10), "error");
    }

    #[test]
    fn truncation_is_unicode_safe() {
        assert_eq!(
            truncate("安装失败", 2),
            "安装\n…输出已截断，请查看应用日志。"
        );
    }
}

use std::path::PathBuf;

/// Phase 1 在非 Windows 平台不启动 Codex Runtime。
pub async fn discover() -> Result<PathBuf, String> {
    Err("BACKEND_UNAVAILABLE: Codex runtime is unavailable on this platform".into())
}

#[cfg(test)]
mod tests {
    /// 非 Windows 平台必须返回稳定且可识别的 Runtime 不可用诊断。
    #[tokio::test]
    async fn non_windows_discovery_is_explicitly_unavailable() {
        let error = super::discover().await.unwrap_err();
        assert!(error.starts_with("BACKEND_UNAVAILABLE:"));
    }
}

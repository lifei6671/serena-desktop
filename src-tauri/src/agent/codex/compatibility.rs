use super::protocol::{CompatibilityIdentity, ProtocolError, Result};

/// Codex binary compatibility 的精确平台与架构维度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    WindowsX86_64,
    MacosArm64,
    MacosX86_64,
}

/// 一条经过真实 Contract Gate 冻结的兼容性记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompatibilityEntry {
    pub codex_version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub binary_sha256: &'static str,
    pub protocol_schema_sha256: &'static str,
}

pub(crate) const WINDOWS_X86_64: CompatibilityEntry = CompatibilityEntry {
    codex_version: "codex-cli 0.153.4",
    os: "windows",
    arch: "x86_64",
    binary_sha256: "444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B",
    protocol_schema_sha256: "B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978",
};

pub(crate) const MACOS_ARM64: CompatibilityEntry = CompatibilityEntry {
    codex_version: "codex-cli 0.153.4",
    os: "macos",
    arch: "arm64",
    binary_sha256: "B973D440ACAC501FD2594A43E7CA9CE41E0A65B9DFB28D0D7A7837C99E1261E3",
    protocol_schema_sha256: "B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978",
};

/// 返回目标平台唯一允许的精确兼容记录；Intel macOS 明确没有 entry。
pub(crate) fn entry_for(target: Target) -> Option<&'static CompatibilityEntry> {
    match target {
        Target::WindowsX86_64 => Some(&WINDOWS_X86_64),
        Target::MacosArm64 => Some(&MACOS_ARM64),
        Target::MacosX86_64 => None,
    }
}

/// 按平台与架构验证完整 identity，不能仅凭相同 version 放行 binary。
pub(crate) fn check_entry(target: Target, identity: &CompatibilityIdentity) -> Result<()> {
    let Some(entry) = entry_for(target) else {
        return Err(ProtocolError::incompatible(
            "No compatibility entry exists for this platform and architecture",
        ));
    };
    if identity.version != entry.codex_version
        || identity.binary_sha256 != entry.binary_sha256
        || identity.protocol_schema_sha256 != entry.protocol_schema_sha256
    {
        return Err(ProtocolError::incompatible(
            "Version, target, binary digest or freshly exported schema digest is not whitelisted",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows 既有 entry 必须继续接受原始 version、binary 与 schema 组合。
    #[test]
    fn windows_entry_preserves_existing_identity() {
        assert!(check_entry(Target::WindowsX86_64, &windows_identity()).is_ok());
    }

    /// macOS ARM64 必须使用独立于 Windows 的精确 binary hash。
    #[test]
    fn macos_arm64_entry_uses_platform_binary_hash() {
        assert!(check_entry(Target::MacosArm64, &macos_identity()).is_ok());
        assert_eq!(
            check_entry(Target::MacosArm64, &windows_identity())
                .unwrap_err()
                .code,
            "CODEX_APP_SERVER_INCOMPATIBLE"
        );
    }

    /// 首版不为 Intel macOS 或 Rosetta Codex 提供 allowlist entry。
    #[test]
    fn macos_x86_64_has_no_entry() {
        assert!(entry_for(Target::MacosX86_64).is_none());
    }

    /// 构造冻结的 Windows 兼容身份。
    fn windows_identity() -> CompatibilityIdentity {
        CompatibilityIdentity {
            version: "codex-cli 0.153.4".into(),
            binary_sha256: "444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B"
                .into(),
            protocol_schema_sha256:
                "B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978".into(),
        }
    }

    /// 构造冻结的 macOS ARM64 兼容身份。
    fn macos_identity() -> CompatibilityIdentity {
        CompatibilityIdentity {
            version: "codex-cli 0.153.4".into(),
            binary_sha256: "B973D440ACAC501FD2594A43E7CA9CE41E0A65B9DFB28D0D7A7837C99E1261E3"
                .into(),
            protocol_schema_sha256:
                "B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978".into(),
        }
    }
}

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::fmt;

/// 内联行级变更正文的 Server hard byte limit。
pub(crate) const INLINE_MUTATION_CONTENT_MAX: usize = 1024 * 1024;
/// 整文件覆盖正文的 Server hard byte limit。
pub(crate) const WHOLE_FILE_WRITE_MAX: usize = 8 * 1024 * 1024;
/// 已有目标文本文件允许处理的 Server hard byte limit。
pub(crate) const TARGET_TEXT_FILE_MAX: usize = 8 * 1024 * 1024;
/// 变更后结果文本文件允许产生的 Server hard byte limit。
pub(crate) const RESULT_TEXT_FILE_MAX: usize = 8 * 1024 * 1024;

/// 尚未路由的六个 Source Write Tool 的稳定 domain identity。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "六个未公开 Source Write 的冻结 domain identity 仅由契约回归保留。"
    )
)]
pub(crate) enum SourceWriteTool {
    #[serde(rename = "source_create_text_file")]
    CreateTextFile,
    #[serde(rename = "source_write_text_file")]
    WriteTextFile,
    #[serde(rename = "source_insert_lines")]
    InsertLines,
    #[serde(rename = "source_delete_lines")]
    DeleteLines,
    #[serde(rename = "source_replace_lines")]
    ReplaceLines,
    #[serde(rename = "source_replace_content")]
    ReplaceContent,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "冻结的 Tool 列表和 wire 名称仅用于未公开 Source Write 的契约回归。"
    )
)]
impl SourceWriteTool {
    /// 以冻结顺序返回全部未来 Tool 名称，仅供内部 domain identity 使用。
    pub(crate) const ALL: [Self; 6] = [
        Self::CreateTextFile,
        Self::WriteTextFile,
        Self::InsertLines,
        Self::DeleteLines,
        Self::ReplaceLines,
        Self::ReplaceContent,
    ];

    /// 返回冻结的 wire Tool 名称。
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::CreateTextFile => "source_create_text_file",
            Self::WriteTextFile => "source_write_text_file",
            Self::InsertLines => "source_insert_lines",
            Self::DeleteLines => "source_delete_lines",
            Self::ReplaceLines => "source_replace_lines",
            Self::ReplaceContent => "source_replace_content",
        }
    }
}

impl fmt::Display for SourceWriteTool {
    /// 将 domain identity 显示为其冻结 wire Tool 名称。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

/// Source Write 的稳定公开错误码；本模块不承担错误到 MCP transport 的投影。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SourceWriteError {
    #[serde(rename = "SOURCE_INVALID_ARGUMENT")]
    InvalidArgument,
    #[serde(rename = "SOURCE_PATH_OUTSIDE_WORKSPACE")]
    PathOutsideWorkspace,
    #[serde(rename = "SOURCE_NOT_FOUND")]
    NotFound,
    #[serde(rename = "SOURCE_IO_ERROR")]
    IoError,
    #[serde(rename = "SOURCE_OUTPUT_LIMIT_EXCEEDED")]
    OutputLimitExceeded,
    #[serde(rename = "SOURCE_INPUT_LIMIT_EXCEEDED")]
    InputLimitExceeded,
    #[serde(rename = "SOURCE_FILE_TOO_LARGE")]
    FileTooLarge,
    #[serde(rename = "SOURCE_BINARY_REJECTED")]
    BinaryRejected,
    #[serde(rename = "SOURCE_RANGE_INVALID")]
    RangeInvalid,
    #[serde(rename = "SOURCE_ALREADY_EXISTS")]
    AlreadyExists,
    #[serde(rename = "SOURCE_VERSION_REQUIRED")]
    VersionRequired,
    #[serde(rename = "SOURCE_VERSION_CONFLICT")]
    VersionConflict,
    #[serde(rename = "SOURCE_COMMIT_STATE_UNKNOWN")]
    CommitStateUnknown,
    #[serde(rename = "SOURCE_CONTENT_NOT_FOUND")]
    ContentNotFound,
    #[serde(rename = "SOURCE_CONTENT_AMBIGUOUS")]
    ContentAmbiguous,
    #[serde(rename = "SOURCE_WRITE_REMOTE_DISABLED")]
    WriteRemoteDisabled,
}

impl SourceWriteError {
    /// 返回冻结的错误码字符串，避免各后续 handler 自行拼写。
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::InvalidArgument => "SOURCE_INVALID_ARGUMENT",
            Self::PathOutsideWorkspace => "SOURCE_PATH_OUTSIDE_WORKSPACE",
            Self::NotFound => "SOURCE_NOT_FOUND",
            Self::IoError => "SOURCE_IO_ERROR",
            Self::OutputLimitExceeded => "SOURCE_OUTPUT_LIMIT_EXCEEDED",
            Self::InputLimitExceeded => "SOURCE_INPUT_LIMIT_EXCEEDED",
            Self::FileTooLarge => "SOURCE_FILE_TOO_LARGE",
            Self::BinaryRejected => "SOURCE_BINARY_REJECTED",
            Self::RangeInvalid => "SOURCE_RANGE_INVALID",
            Self::AlreadyExists => "SOURCE_ALREADY_EXISTS",
            Self::VersionRequired => "SOURCE_VERSION_REQUIRED",
            Self::VersionConflict => "SOURCE_VERSION_CONFLICT",
            Self::CommitStateUnknown => "SOURCE_COMMIT_STATE_UNKNOWN",
            Self::ContentNotFound => "SOURCE_CONTENT_NOT_FOUND",
            Self::ContentAmbiguous => "SOURCE_CONTENT_AMBIGUOUS",
            Self::WriteRemoteDisabled => "SOURCE_WRITE_REMOTE_DISABLED",
        }
    }
}

impl fmt::Display for SourceWriteError {
    /// 错误展示与 serialization 共用唯一冻结 wire 值。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for SourceWriteError {}

/// Source Write 的显式 Workspace authority；不接受 caller-provided root。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceWriteTarget {
    pub(crate) workspace_id: String,
    #[serde(rename = "relative_path")]
    pub(crate) relative_path: String,
}

impl SourceWriteTarget {
    /// 只验证 domain 可判定的必填路径，不复制 WorkspacePathResolver 的边界职责。
    pub(crate) fn validate(&self) -> Result<(), SourceWriteError> {
        if self.relative_path.is_empty() {
            return Err(SourceWriteError::InvalidArgument);
        }
        Ok(())
    }
}

/// 已有文件变更使用的 Read-side SHA-256 version token。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct ExpectedSha256(String);

impl ExpectedSha256 {
    /// 从严格小写、64 位十六进制的公开 wire 值构造版本 token。
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, SourceWriteError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(SourceWriteError::InvalidArgument);
        }
        Ok(Self(value))
    }

    /// 返回可用于 JSON 或日志边界的冻结小写 token。
    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ExpectedSha256 {
    /// 反序列化即执行纯 token 校验，不依赖文件或 Workspace 状态。
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// 修改已有文件共用的 authority 与 version token 输入形状。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VersionedSourceWriteTarget {
    pub(crate) workspace_id: String,
    #[serde(rename = "relative_path")]
    pub(crate) relative_path: String,
    pub(crate) expected_sha256: ExpectedSha256,
}

impl VersionedSourceWriteTarget {
    /// 先完成所有纯 domain 校验，后续 handler 才可解析路径或触碰文件。
    pub(crate) fn validate(&self) -> Result<(), SourceWriteError> {
        self.target().validate()
    }

    /// 构造独立的 Workspace authority DTO，不改变 versioned wire 输入形状。
    pub(crate) fn target(&self) -> SourceWriteTarget {
        SourceWriteTarget {
            workspace_id: self.workspace_id.clone(),
            relative_path: self.relative_path.clone(),
        }
    }

    /// 消费 versioned 输入并取出 Workspace authority，供后续纯校验与路径解析复用。
    #[cfg(test)]
    pub(crate) fn into_target(self) -> SourceWriteTarget {
        SourceWriteTarget {
            workspace_id: self.workspace_id,
            relative_path: self.relative_path,
        }
    }
}

/// 统一的 1-based inclusive 闭区间；文件长度上限语义由具体 handler 决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceLineRange {
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
}

impl SourceLineRange {
    /// 验证与具体文件内容无关的闭区间不变量。
    pub(crate) fn validate(&self) -> Result<(), SourceWriteError> {
        if self.start_line == 0 || self.end_line == 0 || self.start_line > self.end_line {
            return Err(SourceWriteError::RangeInvalid);
        }
        Ok(())
    }
}

/// 拒绝 Rust/JSON String 中可表达的 NUL，避免后续写入把二进制当作文本。
pub(crate) fn validate_text_content(content: &str) -> Result<(), SourceWriteError> {
    if content.as_bytes().contains(&0) {
        return Err(SourceWriteError::BinaryRejected);
    }
    Ok(())
}

/// 校验行级 mutation 正文的 UTF-8 byte budget。
pub(crate) fn validate_inline_mutation_content(content: &str) -> Result<(), SourceWriteError> {
    validate_text_content(content)?;
    validate_input_size(content.len(), INLINE_MUTATION_CONTENT_MAX)
}

/// 校验整文件写入正文的 UTF-8 byte budget。
pub(crate) fn validate_whole_file_write_content(content: &str) -> Result<(), SourceWriteError> {
    validate_text_content(content)?;
    validate_input_size(content.len(), WHOLE_FILE_WRITE_MAX)
}

/// 校验已读取目标文件的 raw bytes 是否仍可作为 UTF-8 文本处理，绝不 lossy conversion。
pub(crate) fn validate_target_text_bytes(bytes: &[u8]) -> Result<&str, SourceWriteError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SourceWriteError::BinaryRejected)?;
    validate_text_content(text)?;
    Ok(text)
}

/// 校验既有 target 文件大小，不读取文件或访问 metadata。
pub(crate) fn validate_target_text_file_size(byte_len: usize) -> Result<(), SourceWriteError> {
    validate_file_size(byte_len, TARGET_TEXT_FILE_MAX)
}

/// 校验将要产生的 result 文件大小，不写入文件。
pub(crate) fn validate_result_text_file_size(byte_len: usize) -> Result<(), SourceWriteError> {
    validate_file_size(byte_len, RESULT_TEXT_FILE_MAX)
}

/// 统一 caller-provided content 的 hard limit 投影。
fn validate_input_size(byte_len: usize, limit: usize) -> Result<(), SourceWriteError> {
    if byte_len > limit {
        return Err(SourceWriteError::InputLimitExceeded);
    }
    Ok(())
}

/// 统一 target/result file 的 hard limit 投影。
fn validate_file_size(byte_len: usize, limit: usize) -> Result<(), SourceWriteError> {
    if byte_len > limit {
        return Err(SourceWriteError::FileTooLarge);
    }
    Ok(())
}

/// 所有六个 Tool 共用的成功结果；此 DTO 只冻结序列化形状，不执行写入。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceWriteSuccess {
    pub(crate) path: String,
    pub(crate) workspace_id: String,
    pub(crate) generation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) before_sha256: Option<ExpectedSha256>,
    pub(crate) after_sha256: ExpectedSha256,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) changed_range: Option<SourceLineRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) changed_count: Option<u32>,
}

impl SourceWriteSuccess {
    /// 验证成功 DTO 内可由本层确定的路径与 range 不变量。
    pub(crate) fn validate(&self) -> Result<(), SourceWriteError> {
        if self.path.is_empty() {
            return Err(SourceWriteError::InvalidArgument);
        }
        if let Some(range) = self.changed_range {
            range.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::registry;
    use serde_json::json;
    use std::cell::Cell;

    /// 生成指定 UTF-8 byte 数的多字节正文，证明所有 limit 均按 byte 而非 char 计算。
    fn utf8_bytes(byte_len: usize) -> String {
        format!("{}{}", "中".repeat(byte_len / 3), "a".repeat(byte_len % 3))
    }

    /// 覆盖技术设计 §50 中全部稳定 Source Write error wire 值。
    #[test]
    fn source_write_errors_keep_all_frozen_wire_values() {
        let cases = [
            (SourceWriteError::InvalidArgument, "SOURCE_INVALID_ARGUMENT"),
            (
                SourceWriteError::PathOutsideWorkspace,
                "SOURCE_PATH_OUTSIDE_WORKSPACE",
            ),
            (SourceWriteError::NotFound, "SOURCE_NOT_FOUND"),
            (SourceWriteError::IoError, "SOURCE_IO_ERROR"),
            (
                SourceWriteError::OutputLimitExceeded,
                "SOURCE_OUTPUT_LIMIT_EXCEEDED",
            ),
            (
                SourceWriteError::InputLimitExceeded,
                "SOURCE_INPUT_LIMIT_EXCEEDED",
            ),
            (SourceWriteError::FileTooLarge, "SOURCE_FILE_TOO_LARGE"),
            (SourceWriteError::BinaryRejected, "SOURCE_BINARY_REJECTED"),
            (SourceWriteError::RangeInvalid, "SOURCE_RANGE_INVALID"),
            (SourceWriteError::AlreadyExists, "SOURCE_ALREADY_EXISTS"),
            (SourceWriteError::VersionRequired, "SOURCE_VERSION_REQUIRED"),
            (SourceWriteError::VersionConflict, "SOURCE_VERSION_CONFLICT"),
            (
                SourceWriteError::CommitStateUnknown,
                "SOURCE_COMMIT_STATE_UNKNOWN",
            ),
            (
                SourceWriteError::ContentNotFound,
                "SOURCE_CONTENT_NOT_FOUND",
            ),
            (
                SourceWriteError::ContentAmbiguous,
                "SOURCE_CONTENT_AMBIGUOUS",
            ),
            (
                SourceWriteError::WriteRemoteDisabled,
                "SOURCE_WRITE_REMOTE_DISABLED",
            ),
        ];
        for (error, wire) in cases {
            assert_eq!(error.code(), wire);
            assert_eq!(error.to_string(), wire);
            assert_eq!(serde_json::to_value(error).unwrap(), json!(wire));
            assert_eq!(
                serde_json::from_value::<SourceWriteError>(json!(wire)).unwrap(),
                error
            );
        }
    }

    /// 六个名称仅是 domain identity，不能成为 registry descriptor 或 dispatch 入口。
    #[test]
    fn source_write_tools_remain_unregistered_and_unavailable() {
        let tools = registry::list(true);
        for tool in SourceWriteTool::ALL {
            assert_eq!(tool.to_string(), tool.code());
            assert!(
                !tools
                    .iter()
                    .any(|descriptor| descriptor.name == tool.code())
            );
            assert_eq!(
                registry::validate(tool.code(), &json!({})),
                Err("UNKNOWN_TOOL".into())
            );
        }
    }

    /// Target authority serialization 保持 workspaceId 加 relative_path，并拒绝没有公开字段的输入。
    #[test]
    fn source_write_target_serialization_keeps_frozen_authority_field_names() {
        let serialized = serde_json::to_value(SourceWriteTarget {
            workspace_id: "workspace-a".into(),
            relative_path: "src/lib.rs".into(),
        })
        .unwrap();
        assert_eq!(
            serialized,
            json!({"workspaceId":"workspace-a", "relative_path":"src/lib.rs"})
        );
        let versioned = VersionedSourceWriteTarget {
            workspace_id: "workspace-a".into(),
            relative_path: "src/lib.rs".into(),
            expected_sha256: ExpectedSha256::parse("a".repeat(64)).unwrap(),
        };
        assert_eq!(
            serde_json::to_value(versioned).unwrap(),
            json!({
                "workspaceId":"workspace-a", "relative_path":"src/lib.rs",
                "expectedSha256":"a".repeat(64)
            })
        );
        assert!(
            serde_json::from_value::<SourceWriteTarget>(json!({
                "workspaceId":"workspace-a", "relative_path":"src/lib.rs"
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<SourceWriteTarget>(json!({
                "workspaceId":"workspace-a", "relative_path":"src/lib.rs", "root":"C:/caller-root"
            }))
            .is_err()
        );
    }

    /// Versioned DTO 直接冻结三字段 wire shape，并真实验证所有拒绝分支。
    #[test]
    fn versioned_source_write_target_deserialization_is_explicit_and_closed() {
        let valid = json!({
            "workspaceId":"workspace-a", "relative_path":"src/lib.rs",
            "expectedSha256":"a".repeat(64)
        });
        let parsed = serde_json::from_value::<VersionedSourceWriteTarget>(valid).unwrap();
        assert_eq!(
            parsed.target(),
            SourceWriteTarget {
                workspace_id: "workspace-a".into(),
                relative_path: "src/lib.rs".into(),
            }
        );
        assert_eq!(
            parsed.into_target(),
            SourceWriteTarget {
                workspace_id: "workspace-a".into(),
                relative_path: "src/lib.rs".into(),
            }
        );
        for invalid in [
            json!({
                "workspaceId":"workspace-a", "relative_path":"src/lib.rs",
                "expectedSha256":"a".repeat(64), "root":"C:/caller-root"
            }),
            json!({"workspaceId":"workspace-a", "relative_path":"src/lib.rs"}),
            json!({
                "workspaceId":"workspace-a", "relative_path":"src/lib.rs",
                "expectedSha256":"A".repeat(64)
            }),
        ] {
            assert!(serde_json::from_value::<VersionedSourceWriteTarget>(invalid).is_err());
        }
    }

    /// expectedSha256 与 Read-side 输出一致，只接受小写 64 位 hex。
    #[test]
    fn expected_sha256_is_strict_lowercase_hex() {
        let valid = "a".repeat(64);
        assert_eq!(
            ExpectedSha256::parse(valid.clone()).unwrap().as_str(),
            valid
        );
        for invalid in [
            "a".repeat(63),
            "a".repeat(65),
            format!("{}A", "a".repeat(63)),
            format!("{}g", "a".repeat(63)),
        ] {
            assert_eq!(
                ExpectedSha256::parse(invalid),
                Err(SourceWriteError::InvalidArgument)
            );
        }
        assert!(serde_json::from_value::<ExpectedSha256>(json!("A".repeat(64))).is_err());
    }

    /// 1-based inclusive 区间只冻结本层不变量，不读取文件长度。
    #[test]
    fn source_line_range_is_one_based_and_inclusive() {
        for range in [
            SourceLineRange {
                start_line: 1,
                end_line: 1,
            },
            SourceLineRange {
                start_line: 1,
                end_line: 8,
            },
        ] {
            assert_eq!(range.validate(), Ok(()));
        }
        for range in [
            SourceLineRange {
                start_line: 0,
                end_line: 1,
            },
            SourceLineRange {
                start_line: 1,
                end_line: 0,
            },
            SourceLineRange {
                start_line: 3,
                end_line: 2,
            },
        ] {
            assert_eq!(range.validate(), Err(SourceWriteError::RangeInvalid));
        }
    }

    /// 所有正文和 raw target 检查拒绝 NUL 或无效 UTF-8，且多字节 UTF-8 保持原样。
    #[test]
    fn text_validators_reject_binary_without_lossy_conversion() {
        assert_eq!(
            validate_text_content("a\0b"),
            Err(SourceWriteError::BinaryRejected)
        );
        assert_eq!(
            validate_inline_mutation_content("a\0b"),
            Err(SourceWriteError::BinaryRejected)
        );
        assert_eq!(
            validate_target_text_bytes(&[0xff]),
            Err(SourceWriteError::BinaryRejected)
        );
        assert_eq!(
            validate_target_text_bytes(b"a\0b"),
            Err(SourceWriteError::BinaryRejected)
        );
        assert_eq!(validate_target_text_bytes("中文".as_bytes()), Ok("中文"));
    }

    /// Inline 与 whole-file limits 使用 UTF-8 byte length，并精确覆盖边界值。
    #[test]
    fn input_limits_are_byte_exact_at_one_and_eight_mib() {
        let inline_at_limit = utf8_bytes(INLINE_MUTATION_CONTENT_MAX);
        let inline_over_limit = utf8_bytes(INLINE_MUTATION_CONTENT_MAX + 1);
        assert_eq!(inline_at_limit.len(), INLINE_MUTATION_CONTENT_MAX);
        assert_eq!(inline_over_limit.len(), INLINE_MUTATION_CONTENT_MAX + 1);
        assert_eq!(validate_inline_mutation_content(&inline_at_limit), Ok(()));
        assert_eq!(
            validate_inline_mutation_content(&inline_over_limit),
            Err(SourceWriteError::InputLimitExceeded)
        );

        let whole_at_limit = utf8_bytes(WHOLE_FILE_WRITE_MAX);
        let whole_over_limit = utf8_bytes(WHOLE_FILE_WRITE_MAX + 1);
        assert_eq!(whole_at_limit.len(), WHOLE_FILE_WRITE_MAX);
        assert_eq!(whole_over_limit.len(), WHOLE_FILE_WRITE_MAX + 1);
        assert_eq!(validate_whole_file_write_content(&whole_at_limit), Ok(()));
        assert_eq!(
            validate_whole_file_write_content(&whole_over_limit),
            Err(SourceWriteError::InputLimitExceeded)
        );
    }

    /// Target/result file limits 不读取文件，只将 caller 提供的 byte length 映射到稳定错误。
    #[test]
    fn file_limits_are_byte_exact_at_eight_mib() {
        for validator in [
            validate_target_text_file_size,
            validate_result_text_file_size,
        ] {
            assert_eq!(validator(8 * 1024 * 1024), Ok(()));
            assert_eq!(
                validator(8 * 1024 * 1024 + 1),
                Err(SourceWriteError::FileTooLarge)
            );
        }
    }

    /// 路径 validator 只拒绝空字符串；测试中的假文件回调不会因非法输入被触发。
    #[test]
    fn path_validation_is_non_empty_only_and_precedes_any_filesystem_callback() {
        let empty_target = SourceWriteTarget {
            workspace_id: "workspace-a".into(),
            relative_path: String::new(),
        };
        let touched = Cell::new(false);
        let filesystem_callback = || touched.set(true);

        if empty_target.validate().is_ok() {
            filesystem_callback();
        }
        assert_eq!(
            empty_target.validate(),
            Err(SourceWriteError::InvalidArgument)
        );
        assert!(!touched.get());

        let whitespace_target = SourceWriteTarget {
            workspace_id: "workspace-a".into(),
            relative_path: "  ".into(),
        };
        assert_eq!(whitespace_target.validate(), Ok(()));

        let hash = ExpectedSha256::parse("a".repeat(64)).unwrap();
        let whitespace_path = SourceWriteSuccess {
            path: "  ".into(),
            workspace_id: "workspace-a".into(),
            generation: 7,
            before_sha256: None,
            after_sha256: hash.clone(),
            changed_range: None,
            changed_count: None,
        };
        assert_eq!(whitespace_path.validate(), Ok(()));
        let empty_path = SourceWriteSuccess {
            path: String::new(),
            ..whitespace_path
        };
        assert_eq!(
            empty_path.validate(),
            Err(SourceWriteError::InvalidArgument)
        );
    }

    /// 成功 DTO 固定 camelCase、可选 before hash 与统一 changedRange/changedCount 表达。
    #[test]
    fn source_write_success_serialization_is_stable_and_closed() {
        let after = ExpectedSha256::parse("b".repeat(64)).unwrap();
        let without_before = SourceWriteSuccess {
            path: "src/new.rs".into(),
            workspace_id: "workspace-a".into(),
            generation: 7,
            before_sha256: None,
            after_sha256: after.clone(),
            changed_range: Some(SourceLineRange {
                start_line: 1,
                end_line: 2,
            }),
            changed_count: Some(2),
        };
        assert_eq!(without_before.validate(), Ok(()));
        let serialized = serde_json::to_value(&without_before).unwrap();
        assert_eq!(serialized["path"], "src/new.rs");
        assert_eq!(serialized["workspaceId"], "workspace-a");
        assert_eq!(serialized["generation"], 7);
        assert!(serialized.get("beforeSha256").is_none());
        assert_eq!(serialized["afterSha256"], "b".repeat(64));
        assert_eq!(
            serialized["changedRange"],
            json!({"startLine":1,"endLine":2})
        );
        assert_eq!(serialized["changedCount"], 2);

        let with_before = SourceWriteSuccess {
            before_sha256: Some(ExpectedSha256::parse("a".repeat(64)).unwrap()),
            ..without_before
        };
        assert_eq!(
            serde_json::to_value(with_before).unwrap()["beforeSha256"],
            "a".repeat(64)
        );
        assert!(
            serde_json::from_value::<SourceWriteSuccess>(json!({
                "path":"src/new.rs", "workspaceId":"workspace-a", "generation":7,
                "afterSha256":"b".repeat(64), "unknown":true
            }))
            .is_err()
        );
    }
}

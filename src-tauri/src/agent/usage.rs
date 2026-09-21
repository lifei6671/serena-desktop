//! 公共 Usage telemetry domain；不包含任何 Provider-private runtime identity。

use std::{error::Error, fmt};

use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

use super::provider::ProviderId;

/// Usage wire 校验失败时对外稳定暴露的错误码。
pub const USAGE_EVENT_INVALID: &str = "USAGE_EVENT_INVALID";

/// 公共 Usage wire 校验错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageValidationError;

impl UsageValidationError {
    /// 返回不随 serde 细节变化的公共错误码。
    pub const fn code(self) -> &'static str {
        USAGE_EVENT_INVALID
    }
}

impl fmt::Display for UsageValidationError {
    /// 将错误格式化为稳定的公共错误码。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(USAGE_EVENT_INVALID)
    }
}

impl Error for UsageValidationError {}

/// Provider-agnostic 的 Usage 完整性。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageCompleteness {
    /// 尚无可信的 Execution Usage。
    Unknown,
    /// 已有可信 Usage，但 Provider 尚未证明其完整性。
    Partial,
    /// Provider 合同已证明这是 authoritative final Usage。
    Complete,
}

/// 公共 Execution Usage 快照。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageSnapshot {
    /// 产生 Usage 的 Provider。
    pub provider_id: ProviderId,
    /// 绑定 Usage 的公共 Execution identity。
    #[serde(deserialize_with = "deserialize_execution_id")]
    pub execution_id: String,
    /// Provider 报告的输入 token 数。
    #[serde(default, deserialize_with = "deserialize_optional_non_negative_i64")]
    pub input_tokens: Option<i64>,
    /// Provider 报告的缓存输入 token 数。
    #[serde(default, deserialize_with = "deserialize_optional_non_negative_i64")]
    pub cached_input_tokens: Option<i64>,
    /// Provider 报告的缓存写入输入 token 数。
    #[serde(default, deserialize_with = "deserialize_optional_non_negative_i64")]
    pub cache_write_input_tokens: Option<i64>,
    /// Provider 报告的输出 token 数。
    #[serde(default, deserialize_with = "deserialize_optional_non_negative_i64")]
    pub output_tokens: Option<i64>,
    /// Provider 报告的 reasoning token 数。
    #[serde(default, deserialize_with = "deserialize_optional_non_negative_i64")]
    pub reasoning_tokens: Option<i64>,
    /// Provider 明确提供的总 token 数，公共层绝不由 breakdown 推导。
    #[serde(default, deserialize_with = "deserialize_optional_non_negative_i64")]
    pub total_tokens: Option<i64>,
    /// Provider 报告的模型上下文窗口大小。
    #[serde(default, deserialize_with = "deserialize_optional_non_negative_i64")]
    pub model_context_window: Option<i64>,
    /// Provider 合同声明的 Usage 完整性。
    pub completeness: UsageCompleteness,
    /// 仅用于 public telemetry freshness 的递增版本。
    #[serde(deserialize_with = "deserialize_u64")]
    pub revision: u64,
    /// Provider 传入的更新时间戳；允许完整 i64 范围。
    #[serde(deserialize_with = "deserialize_i64")]
    pub updated_at: i64,
}

impl UsageSnapshot {
    /// 从 JSON 值解析快照，并将所有 wire 校验失败映射为稳定错误码。
    pub fn from_json(value: &Value) -> Result<Self, UsageValidationError> {
        serde_json::from_value(value.clone()).map_err(|_| UsageValidationError)
    }
}

/// 校验 executionId，避免 public identity 带入空白或控制字符。
fn deserialize_execution_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(de::Error::custom(
            "execution id must not be empty or contain whitespace or control characters",
        ));
    }
    Ok(value)
}

/// 解析 nullable token 或窗口字段，且只接受非负的完整 i64 JSON 整数。
fn deserialize_optional_non_negative_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Value::Number(number)) => number
            .as_i64()
            .filter(|number| *number >= 0)
            .map(Some)
            .ok_or_else(|| de::Error::custom("expected a non-negative i64 integer")),
        Some(_) => Err(de::Error::custom("expected a non-negative i64 integer")),
    }
}

/// 解析 revision，且只接受可完整表示的 u64 JSON 整数。
fn deserialize_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| de::Error::custom("expected a u64 integer")),
        _ => Err(de::Error::custom("expected a u64 integer")),
    }
}

/// 解析 updatedAt，且只接受完整 i64 范围内的 JSON 整数。
fn deserialize_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::Number(number) => number
            .as_i64()
            .ok_or_else(|| de::Error::custom("expected an i64 integer")),
        _ => Err(de::Error::custom("expected an i64 integer")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{USAGE_EVENT_INVALID, UsageCompleteness, UsageSnapshot};

    const OPTIONAL_NUMERIC_FIELDS: [&str; 7] = [
        "inputTokens",
        "cachedInputTokens",
        "cacheWriteInputTokens",
        "outputTokens",
        "reasoningTokens",
        "totalTokens",
        "modelContextWindow",
    ];

    /// 构造包含全部必填及 optional 字段的合法 Usage wire 值。
    fn valid_snapshot_json() -> Value {
        json!({
            "providerId": "codex",
            "executionId": "execution-1",
            "inputTokens": 11,
            "cachedInputTokens": 12,
            "cacheWriteInputTokens": 13,
            "outputTokens": 14,
            "reasoningTokens": 15,
            "totalTokens": 16,
            "modelContextWindow": 17,
            "completeness": "partial",
            "revision": 18,
            "updatedAt": 19
        })
    }

    /// 将非法 wire 值断言为稳定的公共错误码。
    fn assert_invalid(value: Value) {
        let error = UsageSnapshot::from_json(&value).unwrap_err();
        assert_eq!(error.code(), USAGE_EVENT_INVALID);
        assert_eq!(error.to_string(), USAGE_EVENT_INVALID);
    }

    /// 通过字符串解析超过 Rust 字面量边界的 JSON 数字。
    fn json_number(literal: &str) -> Value {
        serde_json::from_str(literal).unwrap()
    }

    /// 全字段正整数应保留原值。
    #[test]
    fn parses_all_positive_integer_fields() {
        let parsed = UsageSnapshot::from_json(&valid_snapshot_json()).unwrap();

        assert_eq!(parsed.input_tokens, Some(11));
        assert_eq!(parsed.cached_input_tokens, Some(12));
        assert_eq!(parsed.cache_write_input_tokens, Some(13));
        assert_eq!(parsed.output_tokens, Some(14));
        assert_eq!(parsed.reasoning_tokens, Some(15));
        assert_eq!(parsed.total_tokens, Some(16));
        assert_eq!(parsed.model_context_window, Some(17));
        assert_eq!(parsed.revision, 18);
        assert_eq!(parsed.updated_at, 19);
    }

    /// 显式 null 应保持 unknown，而不是转换为零。
    #[test]
    fn preserves_explicit_null_optional_fields() {
        let mut value = valid_snapshot_json();
        for field in OPTIONAL_NUMERIC_FIELDS {
            value[field] = Value::Null;
        }

        let parsed = UsageSnapshot::from_json(&value).unwrap();
        assert_eq!(parsed.input_tokens, None);
        assert_eq!(parsed.cached_input_tokens, None);
        assert_eq!(parsed.cache_write_input_tokens, None);
        assert_eq!(parsed.output_tokens, None);
        assert_eq!(parsed.reasoning_tokens, None);
        assert_eq!(parsed.total_tokens, None);
        assert_eq!(parsed.model_context_window, None);
    }

    /// 缺失 optional 字段应保持 unknown，而不是转换为零。
    #[test]
    fn preserves_absent_optional_fields() {
        let mut value = valid_snapshot_json();
        let object = value.as_object_mut().unwrap();
        for field in OPTIONAL_NUMERIC_FIELDS {
            object.remove(field);
        }

        let parsed = UsageSnapshot::from_json(&value).unwrap();
        assert_eq!(parsed.input_tokens, None);
        assert_eq!(parsed.total_tokens, None);
        assert_eq!(parsed.model_context_window, None);
    }

    /// 已知零必须在解析和序列化后仍然是零。
    #[test]
    fn preserves_known_zero() {
        let mut value = valid_snapshot_json();
        for field in OPTIONAL_NUMERIC_FIELDS {
            value[field] = json!(0);
        }

        let parsed = UsageSnapshot::from_json(&value).unwrap();
        let wire = serde_json::to_value(parsed).unwrap();
        for field in OPTIONAL_NUMERIC_FIELDS {
            assert_eq!(wire[field], json!(0));
        }
    }

    /// total 为空时不得从完整 breakdown 伪造总量。
    #[test]
    fn keeps_null_total_with_known_breakdown() {
        let mut value = valid_snapshot_json();
        value["totalTokens"] = Value::Null;

        assert_eq!(UsageSnapshot::from_json(&value).unwrap().total_tokens, None);
    }

    /// total 独立于 nullable breakdown 保持 Provider 原值。
    #[test]
    fn keeps_provider_total_with_partial_breakdown() {
        let mut value = valid_snapshot_json();
        value["inputTokens"] = Value::Null;
        value["reasoningTokens"] = Value::Null;
        value["totalTokens"] = json!(999);

        assert_eq!(
            UsageSnapshot::from_json(&value).unwrap().total_tokens,
            Some(999)
        );
    }

    /// 每个 token 或窗口字段都拒绝负数。
    #[test]
    fn rejects_negative_optional_numeric_fields() {
        for field in OPTIONAL_NUMERIC_FIELDS {
            let mut value = valid_snapshot_json();
            value[field] = json!(-1);
            assert_invalid(value);
        }
    }

    /// 每个 token 或窗口 wire 字段都拒绝浮点数、数值字符串和 i64 溢出。
    #[test]
    fn rejects_invalid_token_wire_numbers() {
        for invalid in [
            json!(1.0),
            json!(0.0),
            json!("123"),
            json_number("9223372036854775808"),
        ] {
            for field in OPTIONAL_NUMERIC_FIELDS {
                let mut value = valid_snapshot_json();
                value[field] = invalid.clone();
                assert_invalid(value);
            }
        }
    }

    /// revision 拒绝负数、浮点、字符串和 u64 溢出。
    #[test]
    fn rejects_invalid_revision_wire_numbers() {
        for invalid in [
            json!(-1),
            json!(1.0),
            json!("1"),
            json_number("18446744073709551616"),
        ] {
            let mut value = valid_snapshot_json();
            value["revision"] = invalid;
            assert_invalid(value);
        }
    }

    /// updatedAt 接受负 i64，但拒绝非整数和 i64 溢出。
    #[test]
    fn validates_updated_at_as_strict_i64() {
        let mut value = valid_snapshot_json();
        value["updatedAt"] = json!(-1);
        assert_eq!(UsageSnapshot::from_json(&value).unwrap().updated_at, -1);

        for invalid in [json!(1.0), json!("1"), json_number("9223372036854775808")] {
            let mut value = valid_snapshot_json();
            value["updatedAt"] = invalid;
            assert_invalid(value);
        }
    }

    /// 非法 ProviderId 必须归一为稳定错误码。
    #[test]
    fn rejects_invalid_provider_id_with_stable_error_code() {
        let mut value = valid_snapshot_json();
        value["providerId"] = json!("invalid provider");
        assert_invalid(value);
    }

    /// 空、空白和控制字符 executionId 必须归一为稳定错误码。
    #[test]
    fn rejects_invalid_execution_id_with_stable_error_code() {
        for invalid in ["", "execution id", "execution\u{0007}id"] {
            let mut value = valid_snapshot_json();
            value["executionId"] = json!(invalid);
            assert_invalid(value);
        }
    }

    /// 未知字段必须被 camelCase DTO 拒绝。
    #[test]
    fn rejects_unknown_field() {
        let mut value = valid_snapshot_json();
        value["runtimeInstanceId"] = json!("private");
        assert_invalid(value);
    }

    /// generic completeness 的三个值都可以逐字 round-trip。
    #[test]
    fn round_trips_all_completeness_values() {
        for (wire_value, expected) in [
            ("unknown", UsageCompleteness::Unknown),
            ("partial", UsageCompleteness::Partial),
            ("complete", UsageCompleteness::Complete),
        ] {
            let mut value = valid_snapshot_json();
            value["completeness"] = json!(wire_value);
            let parsed = UsageSnapshot::from_json(&value).unwrap();
            assert_eq!(parsed.completeness, expected);
            assert_eq!(
                serde_json::to_value(parsed).unwrap()["completeness"],
                json!(wire_value)
            );
        }
    }

    /// 未支持的 completeness 值必须被拒绝。
    #[test]
    fn rejects_unsupported_completeness() {
        let mut value = valid_snapshot_json();
        value["completeness"] = json!("estimated");
        assert_invalid(value);
    }

    /// Snapshot serde wire 必须使用 camelCase 字段名。
    #[test]
    fn serializes_snapshot_as_camel_case() {
        let wire = serde_json::to_value(UsageSnapshot::from_json(&valid_snapshot_json()).unwrap())
            .unwrap();
        assert!(wire.get("providerId").is_some());
        assert!(wire.get("executionId").is_some());
        assert!(wire.get("totalTokens").is_some());
        assert!(wire.get("modelContextWindow").is_some());
        assert!(wire.get("updatedAt").is_some());
        assert!(wire.get("provider_id").is_none());
    }
}

//! Codex app-server 兼容性检测；仅验证 Serena 实际依赖的 schema 子集。

use super::protocol::{ProtocolError, Result};
use serde_json::Value;

/// 已验证发布物的身份，仅作为测试夹具和诊断证据，不参与准入判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityFixture {
    pub codex_version: &'static str,
    pub binary_sha256: &'static str,
    pub protocol_schema_sha256: &'static str,
}

pub const WINDOWS_X86_64: CompatibilityFixture = CompatibilityFixture {
    codex_version: "codex-cli 0.153.4",
    binary_sha256: "DD1108F00D2C31F99FF8061A676C9C1B0181381DC82BD818A4A26A898B3E392A",
    protocol_schema_sha256: "350B2AAB3E0374FDFD323F58B8914F11EBE857696C81406DB41646FB15606882",
};

pub const MACOS_ARM64: CompatibilityFixture = CompatibilityFixture {
    codex_version: "codex-cli 0.153.4",
    binary_sha256: "916D8824041228491254F99F64DE22FB25125D41EABFE0F2CA356B86422F9B7B",
    protocol_schema_sha256: "350B2AAB3E0374FDFD323F58B8914F11EBE857696C81406DB41646FB15606882",
};

/// 应用必须能够收发的方法。
struct MethodRequirement {
    union: &'static str,
    method: &'static str,
    has_params: bool,
}

const REQUIRED_METHODS: &[MethodRequirement] = &[
    MethodRequirement {
        union: "ClientRequest",
        method: "initialize",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "thread/start",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "thread/resume",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "thread/read",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "thread/turns/list",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "thread/items/list",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "thread/backgroundTerminals/clean",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "thread/backgroundTerminals/list",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "thread/name/set",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "turn/start",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientRequest",
        method: "turn/interrupt",
        has_params: true,
    },
    MethodRequirement {
        union: "ClientNotification",
        method: "initialized",
        has_params: false,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "error",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "thread/started",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "thread/name/updated",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "thread/tokenUsage/updated",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "turn/started",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "turn/completed",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "item/started",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "item/completed",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "item/agentMessage/delta",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerNotification",
        method: "item/commandExecution/outputDelta",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerRequest",
        method: "item/commandExecution/requestApproval",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerRequest",
        method: "item/fileChange/requestApproval",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerRequest",
        method: "item/permissions/requestApproval",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerRequest",
        method: "item/tool/requestUserInput",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerRequest",
        method: "item/tool/call",
        has_params: true,
    },
    MethodRequirement {
        union: "ServerRequest",
        method: "account/chatgptAuthTokens/refresh",
        has_params: true,
    },
];

/// 应用反序列化或读取的核心字段。
struct FieldRequirement {
    definition: &'static str,
    property: &'static str,
    required: bool,
    expected_type: &'static str,
}

const REQUIRED_FIELDS: &[FieldRequirement] = &[
    FieldRequirement {
        definition: "InitializeResponse",
        property: "userAgent",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "InitializeResponse",
        property: "codexHome",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "InitializeResponse",
        property: "platformFamily",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "InitializeResponse",
        property: "platformOs",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadStartParams",
        property: "cwd",
        required: false,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadStartParams",
        property: "ephemeral",
        required: false,
        expected_type: "boolean",
    },
    FieldRequirement {
        definition: "v2/ThreadStartParams",
        property: "historyMode",
        required: false,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadStartParams",
        property: "approvalPolicy",
        required: false,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadStartParams",
        property: "sandbox",
        required: false,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadStartParams",
        property: "dynamicTools",
        required: false,
        expected_type: "array",
    },
    FieldRequirement {
        definition: "v2/ThreadResumeParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadResumeParams",
        property: "excludeTurns",
        required: false,
        expected_type: "boolean",
    },
    FieldRequirement {
        definition: "v2/ThreadReadParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadReadParams",
        property: "includeTurns",
        required: false,
        expected_type: "boolean",
    },
    FieldRequirement {
        definition: "v2/ThreadTurnsListParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadItemsListParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadBackgroundTerminalsCleanParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadBackgroundTerminalsListParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadSetNameParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadSetNameParams",
        property: "name",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/TurnStartParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/TurnStartParams",
        property: "input",
        required: true,
        expected_type: "array",
    },
    FieldRequirement {
        definition: "v2/TurnInterruptParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/TurnInterruptParams",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/Thread",
        property: "id",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/Thread",
        property: "turns",
        required: true,
        expected_type: "array",
    },
    FieldRequirement {
        definition: "v2/Thread",
        property: "historyMode",
        required: false,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/Turn",
        property: "id",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/Turn",
        property: "items",
        required: true,
        expected_type: "array",
    },
    FieldRequirement {
        definition: "v2/Turn",
        property: "status",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ErrorNotification",
        property: "error",
        required: true,
        expected_type: "object",
    },
    FieldRequirement {
        definition: "v2/ErrorNotification",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ErrorNotification",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ErrorNotification",
        property: "willRetry",
        required: true,
        expected_type: "boolean",
    },
    FieldRequirement {
        definition: "v2/ThreadNameUpdatedNotification",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadNameUpdatedNotification",
        property: "threadName",
        required: false,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ItemStartedNotification",
        property: "item",
        required: true,
        expected_type: "object",
    },
    FieldRequirement {
        definition: "v2/ItemStartedNotification",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ItemStartedNotification",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ItemCompletedNotification",
        property: "item",
        required: true,
        expected_type: "object",
    },
    FieldRequirement {
        definition: "v2/ItemCompletedNotification",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ItemCompletedNotification",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/AgentMessageDeltaNotification",
        property: "delta",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/AgentMessageDeltaNotification",
        property: "itemId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/AgentMessageDeltaNotification",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/AgentMessageDeltaNotification",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/CommandExecutionOutputDeltaNotification",
        property: "delta",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/CommandExecutionOutputDeltaNotification",
        property: "itemId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/CommandExecutionOutputDeltaNotification",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/CommandExecutionOutputDeltaNotification",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadTokenUsageUpdatedNotification",
        property: "tokenUsage",
        required: true,
        expected_type: "object",
    },
    FieldRequirement {
        definition: "v2/ThreadTokenUsageUpdatedNotification",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadTokenUsageUpdatedNotification",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ThreadTokenUsage",
        property: "last",
        required: true,
        expected_type: "object",
    },
    FieldRequirement {
        definition: "v2/ThreadTokenUsage",
        property: "total",
        required: true,
        expected_type: "object",
    },
    FieldRequirement {
        definition: "v2/TokenUsageBreakdown",
        property: "cachedInputTokens",
        required: true,
        expected_type: "integer",
    },
    FieldRequirement {
        definition: "v2/TokenUsageBreakdown",
        property: "inputTokens",
        required: true,
        expected_type: "integer",
    },
    FieldRequirement {
        definition: "v2/TokenUsageBreakdown",
        property: "outputTokens",
        required: true,
        expected_type: "integer",
    },
    FieldRequirement {
        definition: "v2/TokenUsageBreakdown",
        property: "reasoningOutputTokens",
        required: true,
        expected_type: "integer",
    },
    FieldRequirement {
        definition: "v2/TokenUsageBreakdown",
        property: "totalTokens",
        required: true,
        expected_type: "integer",
    },
    FieldRequirement {
        definition: "v2/CommandExecutionRequestApprovalParams",
        property: "itemId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/CommandExecutionRequestApprovalParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/CommandExecutionRequestApprovalParams",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/FileChangeRequestApprovalParams",
        property: "itemId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/FileChangeRequestApprovalParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/FileChangeRequestApprovalParams",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/PermissionsRequestApprovalParams",
        property: "itemId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/PermissionsRequestApprovalParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/PermissionsRequestApprovalParams",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ToolRequestUserInputParams",
        property: "itemId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ToolRequestUserInputParams",
        property: "questions",
        required: true,
        expected_type: "array",
    },
    FieldRequirement {
        definition: "v2/ToolRequestUserInputParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/ToolRequestUserInputParams",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/DynamicToolCallParams",
        property: "callId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/DynamicToolCallParams",
        property: "threadId",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/DynamicToolCallParams",
        property: "tool",
        required: true,
        expected_type: "string",
    },
    FieldRequirement {
        definition: "v2/DynamicToolCallParams",
        property: "turnId",
        required: true,
        expected_type: "string",
    },
];

const REQUIRED_THREAD_ITEM_VARIANTS: &[&str] = &[
    "agentMessage",
    "reasoning",
    "commandExecution",
    "fileChange",
    "mcpToolCall",
    "dynamicToolCall",
    "subAgentActivity",
    "webSearch",
];

/// 返回 schema 定义；不同 Codex 版本可能保留或折叠 `v2` 命名空间。
fn definition<'a>(schema: &'a Value, path: &str) -> Option<&'a Value> {
    let definitions = schema.get("definitions")?;
    path.split('/')
        .try_fold(definitions, |node, part| node.get(part))
        .or_else(|| definitions.get(path.rsplit('/').next()?))
}

/// 解析本地 JSON Schema 引用；外部引用不属于当前导出文件契约。
fn referenced<'a>(schema: &'a Value, node: &'a Value) -> Option<&'a Value> {
    let pointer = node.get("$ref")?.as_str()?.strip_prefix('#')?;
    schema.pointer(pointer)
}

/// 判断节点是否允许指定 JSON 类型，并展开组合 schema 与本地引用。
fn allows_type(schema: &Value, node: &Value, expected: &str, depth: usize) -> bool {
    if depth > 32 {
        return false;
    }
    if node.get("type").and_then(Value::as_str) == Some(expected)
        || node
            .get("type")
            .and_then(Value::as_array)
            .is_some_and(|types| types.iter().any(|kind| kind.as_str() == Some(expected)))
    {
        return true;
    }
    if let Some(target) = referenced(schema, node) {
        return allows_type(schema, target, expected, depth + 1);
    }
    ["allOf", "anyOf", "oneOf"].iter().any(|keyword| {
        node.get(keyword)
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts
                    .iter()
                    .any(|part| allows_type(schema, part, expected, depth + 1))
            })
    })
}

/// 提取 union 中声明指定 method 的分支。
fn method_branch<'a>(schema: &'a Value, requirement: &MethodRequirement) -> Option<&'a Value> {
    definition(schema, requirement.union)?
        .get("oneOf")?
        .as_array()?
        .iter()
        .find(|branch| {
            let method = branch
                .get("properties")
                .and_then(|value| value.get("method"));
            method.is_some_and(|value| {
                value.get("const").and_then(Value::as_str) == Some(requirement.method)
                    || value
                        .get("enum")
                        .and_then(Value::as_array)
                        .is_some_and(|values| {
                            values
                                .iter()
                                .any(|entry| entry.as_str() == Some(requirement.method))
                        })
            })
        })
}

/// 校验一个方法分支以及其参数 schema 是否仍可解析。
fn validate_method(schema: &Value, requirement: &MethodRequirement) -> Result<()> {
    let branch = method_branch(schema, requirement).ok_or_else(|| {
        ProtocolError::incompatible(format!(
            "Codex app-server schema 缺少必要方法 {}::{}",
            requirement.union, requirement.method
        ))
    })?;
    let required = branch
        .get("required")
        .and_then(Value::as_array)
        .ok_or_else(|| ProtocolError::incompatible("方法分支缺少 required"))?;
    if !required
        .iter()
        .any(|value| value.as_str() == Some("method"))
    {
        return Err(ProtocolError::incompatible(format!(
            "方法 {} 的 method 字段不再是必填项",
            requirement.method
        )));
    }
    if requirement.has_params {
        if !required
            .iter()
            .any(|value| value.as_str() == Some("params"))
        {
            return Err(ProtocolError::incompatible(format!(
                "方法 {} 的 params 字段不再是必填项",
                requirement.method
            )));
        }
        let params = branch
            .get("properties")
            .and_then(|value| value.get("params"))
            .ok_or_else(|| {
                ProtocolError::incompatible(format!(
                    "方法 {} 缺少 params schema",
                    requirement.method
                ))
            })?;
        if !allows_type(schema, params, "object", 0) {
            return Err(ProtocolError::incompatible(format!(
                "方法 {} 的 params schema 不可解析为对象",
                requirement.method
            )));
        }
    }
    Ok(())
}

/// 校验应用读取的字段仍存在，且必填性和可接受类型未收窄。
fn validate_field(schema: &Value, requirement: &FieldRequirement) -> Result<()> {
    let object = definition(schema, requirement.definition).ok_or_else(|| {
        ProtocolError::incompatible(format!(
            "Codex app-server schema 缺少定义 {}",
            requirement.definition
        ))
    })?;
    let property = object
        .get("properties")
        .and_then(|value| value.get(requirement.property))
        .ok_or_else(|| {
            ProtocolError::incompatible(format!(
                "定义 {} 缺少字段 {}",
                requirement.definition, requirement.property
            ))
        })?;
    if requirement.required
        && !object
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| {
                required
                    .iter()
                    .any(|value| value.as_str() == Some(requirement.property))
            })
    {
        return Err(ProtocolError::incompatible(format!(
            "定义 {} 的字段 {} 不再是必填项",
            requirement.definition, requirement.property
        )));
    }
    if !allows_type(schema, property, requirement.expected_type, 0) {
        return Err(ProtocolError::incompatible(format!(
            "定义 {} 的字段 {} 不再兼容 {} 类型",
            requirement.definition, requirement.property, requirement.expected_type
        )));
    }
    Ok(())
}

/// 校验应用处理的 ThreadItem 判别分支仍然存在。
fn validate_thread_item_variant(schema: &Value, expected: &str) -> Result<()> {
    let variants = definition(schema, "v2/ThreadItem")
        .and_then(|value| value.get("oneOf"))
        .and_then(Value::as_array)
        .ok_or_else(|| ProtocolError::incompatible("定义 v2/ThreadItem 缺少 oneOf"))?;
    let exists = variants.iter().any(|variant| {
        let kind = variant
            .get("properties")
            .and_then(|value| value.get("type"));
        kind.is_some_and(|value| {
            value.get("const").and_then(Value::as_str) == Some(expected)
                || value
                    .get("enum")
                    .and_then(Value::as_array)
                    .is_some_and(|values| {
                        values.iter().any(|entry| entry.as_str() == Some(expected))
                    })
        })
    });
    if !exists {
        return Err(ProtocolError::incompatible(format!(
            "定义 v2/ThreadItem 缺少必要分支 {expected}"
        )));
    }
    Ok(())
}

/// 只根据应用实际依赖的协议子集判断导出 schema 是否兼容。
pub(crate) fn validate_schema(bytes: &[u8]) -> Result<()> {
    let schema: Value = serde_json::from_slice(bytes).map_err(|error| {
        ProtocolError::incompatible(format!("Codex app-server schema 不是有效 JSON: {error}"))
    })?;
    for requirement in REQUIRED_METHODS {
        validate_method(&schema, requirement)?;
    }
    for requirement in REQUIRED_FIELDS {
        validate_field(&schema, requirement)?;
    }
    for variant in REQUIRED_THREAD_ITEM_VARIANTS {
        validate_thread_item_variant(&schema, variant)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    /// 按路径创建测试定义，避免复制真实 Codex 的大型 schema。
    fn insert_definition(schema: &mut Value, path: &str, value: Value) {
        let mut node = schema
            .get_mut("definitions")
            .and_then(Value::as_object_mut)
            .expect("definitions object");
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                node.insert(part.to_owned(), value);
                return;
            }
            node = node
                .entry(part)
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .expect("nested definitions object");
        }
    }

    /// 构造覆盖当前兼容性子集的最小 schema。
    fn compatible_schema() -> Value {
        let mut schema = json!({"definitions": {}});
        let mut unions: std::collections::BTreeMap<&str, Vec<Value>> = Default::default();
        for (index, requirement) in REQUIRED_METHODS.iter().enumerate() {
            let mut required = vec![json!("method")];
            let mut properties = json!({
                "method": {"const": requirement.method}
            });
            if requirement.has_params {
                let definition_name = format!("TestParams{index}");
                insert_definition(
                    &mut schema,
                    &definition_name,
                    json!({"type": "object", "properties": {}}),
                );
                required.push(json!("params"));
                properties["params"] = json!({"$ref": format!("#/definitions/{definition_name}")});
            }
            unions
                .entry(requirement.union)
                .or_default()
                .push(json!({"type": "object", "properties": properties, "required": required}));
        }
        for (name, branches) in unions {
            insert_definition(&mut schema, name, json!({"oneOf": branches}));
        }

        let mut objects: std::collections::BTreeMap<&str, (Map<String, Value>, Vec<Value>)> =
            Default::default();
        for requirement in REQUIRED_FIELDS {
            let (properties, required) = objects.entry(requirement.definition).or_default();
            properties.insert(
                requirement.property.to_owned(),
                json!({"type": requirement.expected_type}),
            );
            if requirement.required {
                required.push(json!(requirement.property));
            }
        }
        for (name, (properties, required)) in objects {
            insert_definition(
                &mut schema,
                name,
                json!({"type": "object", "properties": properties, "required": required}),
            );
        }
        insert_definition(
            &mut schema,
            "v2/ThreadItem",
            json!({
                "oneOf": REQUIRED_THREAD_ITEM_VARIANTS
                    .iter()
                    .map(|kind| json!({"properties": {"type": {"const": kind}}}))
                    .collect::<Vec<_>>()
            }),
        );
        schema
    }

    /// 新增方法、字段和分支不应导致兼容性拒绝。
    #[test]
    fn schema_contract_allows_additions() {
        let mut schema = compatible_schema();
        schema["definitions"]["InitializeResponse"]["properties"]["futureField"] =
            json!({"type": "string"});
        assert!(validate_schema(schema.to_string().as_bytes()).is_ok());
    }

    /// 定义名称变化但 params 仍可解析为对象时应保持兼容。
    #[test]
    fn schema_contract_allows_resolved_params_definition_rename() {
        let mut schema = compatible_schema();
        schema["definitions"]["RenamedInitializeParams"] =
            json!({"type": "object", "properties": {}});
        let branch = schema["definitions"]["ClientRequest"]["oneOf"]
            .as_array_mut()
            .expect("request branches")
            .iter_mut()
            .find(|branch| branch["properties"]["method"]["const"].as_str() == Some("initialize"))
            .expect("initialize branch");
        branch["properties"]["params"]["$ref"] = json!("#/definitions/RenamedInitializeParams");
        assert!(validate_schema(schema.to_string().as_bytes()).is_ok());
    }

    /// 缺少应用调用的方法必须拒绝。
    #[test]
    fn schema_contract_rejects_missing_method() {
        let mut schema = compatible_schema();
        schema["definitions"]["ClientRequest"]["oneOf"]
            .as_array_mut()
            .expect("request branches")
            .retain(|branch| {
                branch["properties"]["method"]["const"].as_str() != Some("turn/start")
            });
        assert!(validate_schema(schema.to_string().as_bytes()).is_err());
    }

    /// 不可解析的 params 引用必须拒绝。
    #[test]
    fn schema_contract_rejects_unresolved_params_reference() {
        let mut schema = compatible_schema();
        let branch = schema["definitions"]["ClientRequest"]["oneOf"]
            .as_array_mut()
            .expect("request branches")
            .first_mut()
            .expect("request branch");
        branch["properties"]["params"]["$ref"] = json!("#/definitions/MissingParams");
        assert!(validate_schema(schema.to_string().as_bytes()).is_err());
    }

    /// 核心字段缺失或类型变化必须拒绝。
    #[test]
    fn schema_contract_rejects_changed_core_field() {
        let mut missing = compatible_schema();
        missing["definitions"]["InitializeResponse"]["properties"]
            .as_object_mut()
            .expect("properties")
            .remove("userAgent");
        assert!(validate_schema(missing.to_string().as_bytes()).is_err());

        let mut changed = compatible_schema();
        changed["definitions"]["v2"]["Turn"]["properties"]["items"]["type"] = json!("string");
        assert!(validate_schema(changed.to_string().as_bytes()).is_err());
    }

    /// 缺少应用处理的 ThreadItem 分支必须拒绝。
    #[test]
    fn schema_contract_rejects_missing_thread_item_variant() {
        let mut schema = compatible_schema();
        schema["definitions"]["v2"]["ThreadItem"]["oneOf"]
            .as_array_mut()
            .expect("item variants")
            .retain(|variant| {
                variant["properties"]["type"]["const"].as_str() != Some("agentMessage")
            });
        assert!(validate_schema(schema.to_string().as_bytes()).is_err());
    }

    /// 非 JSON 输入必须作为不兼容处理。
    #[test]
    fn schema_contract_rejects_invalid_json() {
        assert!(validate_schema(b"not json").is_err());
    }
}

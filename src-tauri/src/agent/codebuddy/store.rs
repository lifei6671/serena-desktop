//! CodeBuddy 私有 durable identity；不提供公共投影、发送或 Claim authority。

use crate::agent::store::StateStore;
use agent_client_protocol::schema::v1::{RequestId, StopReason};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 原始 JSON scalar 保留 SDK string/i64 类型；Null 不具备精确请求身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PromptRpcId(RequestId);

impl PromptRpcId {
    /// 仅接受 pinned SDK 的非空请求 ID domain，不扩大为浮点数或 u64。
    pub(crate) fn from_json(json: &str) -> Result<Self, String> {
        match serde_json::from_str(json).map_err(|_| "invalid prompt RPC id")? {
            RequestId::Null => Err("null prompt RPC id".into()),
            id => Ok(Self(id)),
        }
    }

    /// SDK 标量序列化保留字符串引号，避免数字与数字字符串混淆。
    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(&self.0).expect("SDK request id is a JSON scalar")
    }
}

/// 私有 Prompt 状态；prepared 表明本地 Prompt identity 已持久化，但 Runtime/Session 可能尚未 ready。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PromptState {
    Prepared,
    Sent,
    Uncertain,
    TerminalObserved,
}

/// Session 检查来源状态，不存在 recovered_completed。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryState {
    NotAttempted,
    Inspecting,
    Partial,
    Unknown,
    MaterialDifference,
}

/// finish 只接受检查结果，无法传入 lifecycle 完成或 Claim 释放状态。
#[derive(Clone, Copy, Debug)]
pub(crate) enum InspectionOutcome {
    Partial,
    Unknown,
    MaterialDifference,
}

/// 已校验的私有父记录；没有 Serialize，禁止整体转成公共 DTO。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PrivateState {
    pub(crate) execution_id: String,
    pub(crate) runtime_instance_id: Option<String>,
    pub(crate) acp_protocol_version: Option<u16>,
    pub(crate) session_id: Option<String>,
    pub(crate) conversation_request_id: String,
    pub(crate) provider_request_id: Option<String>,
    pub(crate) provider_request_id_source: Option<String>,
    pub(crate) prompt_rpc_id: Option<PromptRpcId>,
    pub(crate) prompt_state: PromptState,
    pub(crate) terminal_stop_reason: Option<StopReason>,
    pub(crate) terminal_observed_at: Option<i64>,
    pub(crate) recovery_method: Option<String>,
    pub(crate) recovery_state: RecoveryState,
    pub(crate) recovery_runtime_instance_id: Option<String>,
    pub(crate) recovery_started_at: Option<i64>,
    pub(crate) recovery_finished_at: Option<i64>,
    pub(crate) revision: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

/// 同时比较 generic revision 与 binding，避免过期调用确认错误的 R1。
#[derive(Clone, Debug)]
pub(crate) struct Ownership {
    pub(crate) execution_revision: i64,
    pub(crate) runtime_instance_id: Option<String>,
}

/// 每种写操作都有明确来源；terminal 要求调用者持有 exact live response 身份。
#[derive(Clone, Debug)]
pub(crate) enum Mutation {
    BindRuntime,
    NegotiatedProtocol(u16),
    ExactSession(String),
    /// Continue child 已验证 load/history 且 durable Sent 后，冻结 source S1 与本 Runtime recovery。
    BeginContinuationLoad {
        session_id: String,
        recovery_runtime_instance_id: String,
    },
    /// exact-S1 非空可用 history 已验证；这里只冻结 partial recovery，不合成 terminal。
    FinishContinuationLoad,
    /// 精确 Provider 观测独立持久化，绝不与本地 conversation identity 自动等同。
    ExactProviderRequest(String),
    MarkSent {
        rpc_id: Option<PromptRpcId>,
    },
    MarkUncertain,
    ObserveTerminal {
        session_id: String,
        conversation_request_id: String,
        stop_reason: StopReason,
        observed_at: i64,
    },
    BeginInspection {
        recovery_runtime_instance_id: String,
    },
    FinishInspection {
        outcome: InspectionOutcome,
    },
}

/// Adapter 持有的窄 typed 入口；SQL 与 Connection 始终留在 agent::store。
pub(crate) struct CodeBuddyStore(pub(crate) StateStore);

impl CodeBuddyStore {
    /// R1 proof 已完成后，原子登记 recovery attempt、R2 runtime 与 private inspection provenance。
    #[allow(
        clippy::too_many_arguments,
        reason = "R1/R2 durable identities stay explicit"
    )]
    pub(crate) async fn begin_result_inspection(
        &self,
        expected: PrivateState,
        ownership: Ownership,
        recovery_runtime_instance_id: String,
        owner: String,
        session: u32,
        executable: String,
    ) -> Result<PrivateState, String> {
        self.0
            .begin_codebuddy_result_inspection(
                expected,
                ownership,
                recovery_runtime_instance_id,
                owner,
                session,
                executable,
            )
            .await
    }

    /// R2 proof 已完成后，原子冻结 inspection outcome 与 generic partial/unknown result。
    pub(crate) async fn finish_result_inspection(
        &self,
        expected: PrivateState,
        ownership: Ownership,
        outcome: InspectionOutcome,
        result: Option<Value>,
    ) -> Result<PrivateState, String> {
        self.0
            .finish_codebuddy_result_inspection(expected, ownership, outcome, result)
            .await
    }

    /// 只读取可作为 Continue source 的 exact private identity；缺失或不合格返回 None。
    pub(crate) async fn continuation_source(
        &self,
        execution_id: String,
    ) -> Result<Option<PrivateState>, String> {
        self.0
            .read_codebuddy_continuation_source(execution_id)
            .await
    }

    /// 原子验证 child/source 冻结 lineage，并返回 child generic authority 与 source private S1。
    pub(crate) async fn continuation_lineage(
        &self,
        execution_id: String,
        source_execution_id: String,
    ) -> Result<Option<(crate::agent::store::ExecutionRecord, PrivateState)>, String> {
        self.0
            .read_codebuddy_continuation_lineage(execution_id, source_execution_id)
            .await
    }

    /// 只合并 exact response 的私有观测；不写 generic terminal，也不释放 Claim。
    pub(crate) async fn observe_prompt_response(
        &self,
        expected: PrivateState,
        provider_request_id: Option<String>,
        stop_reason: StopReason,
        observed_at: i64,
    ) -> Result<PrivateState, String> {
        self.0
            .commit_codebuddy_prompt_response(
                expected,
                provider_request_id,
                stop_reason,
                observed_at,
            )
            .await
    }

    /// 原子预留 UUIDv7 Prompt identity；重复创建明确 conflict，不覆盖旧 identity。
    pub(crate) async fn create(
        &self,
        execution_id: String,
        ownership: Ownership,
    ) -> Result<PrivateState, String> {
        self.0.create_codebuddy_state(execution_id, ownership).await
    }

    /// 缺失历史行返回错误，绝不根据 generic execution 推造身份。
    pub(crate) async fn read(&self, execution_id: String) -> Result<PrivateState, String> {
        self.0.read_codebuddy_state(execution_id).await
    }

    /// 成功返回即表示 FULL synchronous 事务已提交；create 后本地 identity 已 durable；MarkSent 另校验完整 runtime/session。
    pub(crate) async fn mutate(
        &self,
        execution_id: String,
        ownership: Ownership,
        expected_revision: i64,
        mutation: Mutation,
    ) -> Result<PrivateState, String> {
        self.0
            .mutate_codebuddy_state(execution_id, ownership, expected_revision, mutation)
            .await
    }
}

/// 校验完整 UUIDv7 wire 语法、version 与 RFC variant，不宣称验证生成来源。
pub(crate) fn valid_conversation_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 32
        && bytes
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        && bytes[12] == b'7'
        && matches!(bytes[16], b'8' | b'9' | b'a' | b'b')
}

/// SerenaDesktop 在 create 事务内生成 UUIDv7；时间仅用于 UUID 格式，不推断外部身份。
pub(crate) fn new_conversation_id() -> Result<String, String> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "UUIDv7 clock before epoch")?
        .as_millis();
    if millis >= 1u128 << 48 {
        return Err("UUIDv7 clock overflow".into());
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "UUIDv7 entropy unavailable")?;
    bytes[..6].copy_from_slice(&(millis as u64).to_be_bytes()[2..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

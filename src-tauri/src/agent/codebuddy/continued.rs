//! Continue Session 准备边界；只执行 exact source lineage 与唯一 session/load。

use super::{
    discovery::ResolvedLaunchSpec,
    fresh::{
        DesiredConfiguration, PreparedFreshSession, SessionCatalog, StartedSession,
        check_replay_bound, start_owned,
    },
    protocol::{Failure, Limits},
    store::{CodeBuddyStore, Mutation},
};
use crate::agent::store::StateStore;
use agent_client_protocol::schema::v1::{LoadSessionRequest, NewSessionResponse};

/// child 专属 prepare：先冻结 source S1/Workspace lineage，再创建独立 Runtime R2。
#[expect(
    clippy::too_many_arguments,
    reason = "frozen continuation boundary keeps source and child runtime identities explicit"
)]
pub(super) async fn prepare_owned(
    store: StateStore,
    owner: String,
    execution_id: String,
    source_execution_id: String,
    resolved: &ResolvedLaunchSpec,
    desired: DesiredConfiguration,
    limits: Limits,
    runtime_id: String,
) -> Result<PreparedFreshSession, Failure> {
    let private_store = CodeBuddyStore(store.clone());
    let Some((row, source_private)) = private_store
        .continuation_lineage(execution_id.clone(), source_execution_id)
        .await
        .map_err(|_| Failure::State)?
    else {
        return Err(Failure::Continuation);
    };
    let source_session_id = source_private.session_id.ok_or(Failure::Continuation)?;
    let StartedSession {
        runtime,
        handshake,
        mut private,
        ownership,
        cwd,
    } = start_owned(
        store.clone(),
        owner,
        execution_id.clone(),
        row,
        resolved,
        limits,
        runtime_id.clone(),
    )
    .await?;

    let result = async {
        if !handshake.response.agent_capabilities.load_session {
            return Err(Failure::Continuation);
        }
        let requests = &runtime.client.as_ref().ok_or(Failure::Closed)?.requests;
        // route 必须先于 request 发布，保证 response 前 history 也进入 exact-S1 有界队列。
        requests.shared.register_route(&source_session_id)?;
        private = private_store
            .mutate(
                execution_id.clone(),
                ownership.clone(),
                private.revision,
                Mutation::ExactSession(source_session_id.clone()),
            )
            .await
            .map_err(|_| Failure::State)?;
        // Pinned ACP typed boundary 默认 mcpServers=[]；禁止 session/new/resume 或第二方法 fallback。
        let response = requests
            .request(LoadSessionRequest::new(source_session_id.clone(), cwd))
            .await?;
        let extensions = requests
            .shared
            .take_session_load_extensions(&source_session_id)?;
        let replay = requests
            .shared
            .take_continuation_replay(&source_session_id)?;
        check_replay_bound(&replay, limits)?;
        let mut catalog = SessionCatalog {
            response: NewSessionResponse::new(source_session_id.clone())
                .modes(response.modes)
                .config_options(response.config_options),
            models: extensions.models,
        };
        catalog.replay(&source_session_id, &replay)?;
        // 默认保留 Provider 当前模式；显式 mode/option 才通过 typed ACP 配置。
        catalog.configure(requests, &desired).await?;
        let tail = requests.shared.take_continuation_tail(&source_session_id)?;
        check_replay_bound(&tail, limits)?;
        catalog.replay(&source_session_id, &tail)?;
        catalog.confirm_desired(&desired)?;
        requests.shared.check_expiry()?;
        Ok(catalog)
    }
    .await;

    match result {
        Ok(catalog) => Ok(PreparedFreshSession {
            runtime,
            handshake,
            private,
            catalog,
            // Parent history 已验证后隔离；child result 只收集其自己的 prompt frames。
            early_frames: Vec::new(),
            desired,
            continued: true,
            accepted: false,
        }),
        Err(error) => {
            runtime.shutdown().await?;
            Err(error)
        }
    }
}

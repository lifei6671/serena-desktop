//! Fresh Session 准备边界；不派发 prompt、不释放 Claim、不提供 public ProviderPort。
use super::{
    client::Handshake,
    discovery::ResolvedLaunchSpec,
    platform_launcher::{LaunchRequest, UncCurrentDirectoryPolicy},
    protocol::{Failure, Limits, SessionFrame},
    runtime::Runtime,
    store::{CodeBuddyStore, Mutation, Ownership, PrivateState, new_conversation_id},
};
use crate::agent::{
    coordinator::now,
    execution::ExecutionProfile,
    provider::{
        ExecutionConfigurationCatalog, ExecutionConfigurationOption, ExecutionModelOption,
        ProviderId, port::ProviderAcceptanceSink,
    },
    store::{ExecutionRecord, StateStore},
};
use agent_client_protocol::schema::v1::{
    CurrentModeUpdate, NewSessionRequest, NewSessionResponse, SessionConfigKind,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigOptionValue,
    SessionConfigSelectOptions, SessionId, SetSessionConfigOptionRequest, SetSessionModeRequest,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// 本次 Session 的显式内部配置；未指定 mode/option 时保留 Provider 当前模式。
#[derive(Default)]
pub(crate) struct DesiredConfiguration {
    pub(crate) mode: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning: Option<String>,
    /// 保留内部测试/既有 mode-option 路径；Product profile 永不填充此字段。
    pub(crate) option: Option<(String, SessionConfigOptionValue)>,
}

/// 目录仅存内存；typed modes/config 及 raw 白名单 models 各自保留来源。
pub(crate) struct SessionCatalog {
    pub(crate) response: NewSessionResponse,
    pub(crate) models: Option<Value>,
}

/// ACCEPTANCE_READY 持有整个受管 Runtime；任何未消费 drop 都由 Runtime 清理整 Job。
pub(crate) struct PreparedFreshSession {
    pub(super) runtime: Runtime,
    pub(crate) handshake: Handshake,
    pub(crate) private: PrivateState,
    pub(crate) catalog: SessionCatalog,
    /// 保留原序帧供下一卡 Activity 消费；配置已按原序应用，不串入其他 Session。
    pub(crate) early_frames: Vec<SessionFrame>,
    pub(super) desired: DesiredConfiguration,
    /// Continue replay 只能验证 lineage；acceptance 前的历史帧不得进入 child Activity/result。
    pub(super) continued: bool,
    pub(super) accepted: bool,
}

/// Fresh/Continue 共享到 initialize 为止的受管 Runtime 与 child private binding。
pub(super) struct StartedSession {
    pub(super) runtime: Runtime,
    pub(super) handshake: Handshake,
    pub(super) private: PrivateState,
    pub(super) ownership: Ownership,
    pub(super) cwd: PathBuf,
}

impl PreparedFreshSession {
    /// acceptance 前最后检查 transport；消费式边界不发送任何 prompt。
    pub(crate) fn accept(mut self, sink: &dyn ProviderAcceptanceSink) -> Result<Self, Failure> {
        self.check_acceptance_ready()?;
        self.mark_accepted(sink)?;
        Ok(self)
    }

    /// 单次同步 acceptance；Prompt 调用方必须先完成 durable send-intent。
    pub(super) fn mark_accepted(
        &mut self,
        sink: &dyn ProviderAcceptanceSink,
    ) -> Result<(), Failure> {
        if self.accepted {
            return Err(Failure::State);
        }
        self.accepted = true;
        sink.accepted();
        Ok(())
    }

    /// Prompt send-intent 前复用准备校验，但 acceptance 必须等 durable MarkSent 后发生。
    pub(super) fn check_acceptance_ready(&mut self) -> Result<(), Failure> {
        if self.accepted {
            return Err(Failure::State);
        }
        let shared = &self
            .runtime
            .client
            .as_ref()
            .ok_or(Failure::Closed)?
            .requests
            .shared;
        let session_id = self.catalog.response.session_id.to_string();
        let frames = if self.continued {
            shared.take_continuation_tail(&session_id)?
        } else {
            shared.take_session(&session_id)?
        };
        check_replay_bound(&frames, shared.limits)?;
        self.catalog.replay(&session_id, &frames)?;
        self.catalog.confirm_desired(&self.desired)?;
        if !self.continued {
            self.early_frames.extend(frames);
        }
        check_replay_bound(&self.early_frames, shared.limits)?;
        Ok(())
    }

    /// 显式退出同样走 Job evidence，绝不借 cleanup 释放 Execution/Claim。
    pub(crate) async fn shutdown(self) -> Result<(), Failure> {
        self.runtime.shutdown().await
    }
}

/// 唯一 Workspace authority 来自当前 Execution；外部路径只投影一次。
pub(crate) async fn prepare(
    store: StateStore,
    owner: String,
    execution_id: String,
    resolved: &ResolvedLaunchSpec,
    desired: DesiredConfiguration,
    limits: Limits,
) -> Result<PreparedFreshSession, Failure> {
    let runtime_id = format!(
        "codebuddy-{}",
        new_conversation_id().map_err(|_| Failure::State)?
    );
    prepare_owned(
        store,
        owner,
        execution_id,
        resolved,
        desired,
        limits,
        runtime_id,
    )
    .await
}

/// execute 预先冻结本次 R1，失败后只能收敛这一次已取得的 ownership。
pub(super) async fn prepare_owned(
    store: StateStore,
    owner: String,
    execution_id: String,
    resolved: &ResolvedLaunchSpec,
    desired: DesiredConfiguration,
    limits: Limits,
    runtime_id: String,
) -> Result<PreparedFreshSession, Failure> {
    let row = store
        .execution(execution_id.clone())
        .await
        .map_err(|_| Failure::State)?
        .ok_or(Failure::State)?;
    if row.parent_execution_id.is_some() {
        return Err(Failure::State);
    }
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
        runtime_id,
    )
    .await?;
    let private_store = CodeBuddyStore(store);
    let result = async {
        let requests = &runtime.client.as_ref().ok_or(Failure::Closed)?.requests;
        // SDK 构造默认明确包含空 mcpServers；不附加 systemPrompt 或其它 _meta。
        let response = requests.request(NewSessionRequest::new(cwd)).await?;
        let session_id = response.session_id.to_string();
        let extensions = requests.shared.take_session_new_extensions(&session_id)?;
        private = private_store
            .mutate(
                execution_id,
                ownership,
                private.revision,
                Mutation::ExactSession(session_id.clone()),
            )
            .await
            .map_err(|_| Failure::State)?;
        requests.shared.register_route(&session_id)?;
        let mut early_frames = requests.shared.take_session(&session_id)?;
        let mut catalog = SessionCatalog {
            response,
            models: extensions.models,
        };
        catalog.replay(&session_id, &early_frames)?;
        // 默认保留 Provider 当前模式；显式 mode/option 才通过 typed ACP 配置。
        catalog.configure(requests, &desired).await?;
        let after_config = requests.shared.take_session(&session_id)?;
        catalog.replay(&session_id, &after_config)?;
        early_frames.extend(after_config);
        // 移出 transport queue 的 replay buffer 仍受同一计数/bytes 预算约束。
        check_replay_bound(&early_frames, limits)?;
        catalog.confirm_desired(&desired)?;
        requests.shared.check_expiry()?;
        Ok((catalog, early_frames))
    }
    .await;
    match result {
        Ok((catalog, early_frames)) => Ok(PreparedFreshSession {
            runtime,
            handshake,
            private,
            catalog,
            early_frames,
            desired,
            continued: false,
            accepted: false,
        }),
        Err(error) => {
            // Session/config 副作用未知不重试；失败仍必须提交或尝试 Runtime evidence。
            runtime.shutdown().await?;
            Err(error)
        }
    }
}

/// 只共享 child Runtime 建立；调用方必须先完成 Fresh 或 Continue 专属 lineage 校验。
pub(super) async fn start_owned(
    store: StateStore,
    owner: String,
    execution_id: String,
    row: ExecutionRecord,
    resolved: &ResolvedLaunchSpec,
    limits: Limits,
    runtime_id: String,
) -> Result<StartedSession, Failure> {
    if row.id != execution_id
        || row.provider != "codebuddy"
        || row.runtime_instance_id.is_some()
        || row.status != "dispatch_pending"
        || row.dispatch_state != "not_dispatched"
    {
        return Err(Failure::State);
    }
    let request = LaunchRequest::from_resolved(
        resolved,
        Path::new(&row.canonical_workspace_root),
        UncCurrentDirectoryPolicy::Unsupported,
        runtime_id.clone(),
    )
    .map_err(|_| Failure::Launch)?;
    let cwd = request.projected_cwd().as_path().to_owned();
    #[cfg(windows)]
    let session = super::recovery::current_session().map_err(|_| Failure::Launch)?;
    let private_store = CodeBuddyStore(store.clone());
    let mut private = private_store
        .create(
            execution_id.clone(),
            Ownership {
                execution_revision: row.revision,
                runtime_instance_id: None,
            },
        )
        .await
        .map_err(|_| Failure::State)?;
    // 预留记录使 prepare 与 binding 之间的 crash 也不能被误判为从未尝试 Runtime。
    store
        .reserve_runtime_attempt(execution_id.clone(), runtime_id.clone(), now())
        .await
        .map_err(|_| Failure::State)?;
    #[cfg(windows)]
    store
        .prepare_codebuddy_runtime(
            runtime_id.clone(),
            owner,
            session,
            resolved.executable.to_string_lossy().into_owned(),
            now(),
        )
        .await
        .map_err(|_| Failure::State)?;
    #[cfg(target_os = "macos")]
    store
        .prepare_codebuddy_macos_runtime(
            runtime_id.clone(),
            owner,
            resolved.executable.to_string_lossy().into_owned(),
            now(),
        )
        .await
        .map_err(|_| Failure::State)?;
    let ownership = Ownership {
        execution_revision: row.revision + 1,
        runtime_instance_id: Some(runtime_id.clone()),
    };
    let binding = async {
        store
            .bind_codebuddy_prepared_runtime(
                execution_id.clone(),
                row.revision,
                runtime_id.clone(),
                now(),
            )
            .await
            .map_err(|_| Failure::State)?;
        private_store
            .mutate(
                execution_id.clone(),
                ownership.clone(),
                private.revision,
                Mutation::BindRuntime,
            )
            .await
            .map_err(|_| Failure::State)
    }
    .await;
    private = match binding {
        Ok(private) => private,
        Err(error) => {
            // 尚未 launch；原记录也必须保留 recovery 分类，不伪装成无 Runtime 尝试。
            let _ =
                super::recovery::recover(&store, runtime_id, std::time::Duration::from_secs(10))
                    .await;
            return Err(error);
        }
    };
    let (runtime, handshake) =
        Runtime::start_persisted(request, limits, store.clone(), runtime_id).await?;
    private = match private_store
        .mutate(
            execution_id,
            ownership.clone(),
            private.revision,
            Mutation::NegotiatedProtocol(handshake.response.protocol_version.as_u16()),
        )
        .await
    {
        Ok(private) => private,
        Err(_) => {
            runtime.shutdown().await?;
            return Err(Failure::State);
        }
    };
    Ok(StartedSession {
        runtime,
        handshake,
        private,
        ownership,
        cwd,
    })
}

/// 已消费帧仍在 Prepared 对象内等待下一卡，使用与 transport 相同的总预算。
pub(super) fn check_replay_bound(frames: &[SessionFrame], limits: Limits) -> Result<(), Failure> {
    if frames.len() > limits.queue_count {
        return Err(Failure::QueueCount);
    }
    let bytes = frames.iter().try_fold(0usize, |total, frame| {
        let bytes = serde_json::to_vec(&frame.params)
            .map_err(|_| Failure::Malformed)?
            .len()
            + frame.method.len();
        total.checked_add(bytes).ok_or(Failure::QueueBytes)
    })?;
    if bytes > limits.queue_bytes {
        return Err(Failure::QueueBytes);
    }
    Ok(())
}

impl SessionCatalog {
    /// 仅 typed modes 或 category=mode 的 select 是权限模式 authority，不猜 option id。
    fn advertises_mode(&self, mode: &str) -> bool {
        self.response
            .modes
            .as_ref()
            .is_some_and(|m| m.available_modes.iter().any(|v| v.id.to_string() == mode))
            || self
                .response
                .config_options
                .as_ref()
                .is_some_and(|options| {
                    options.iter().any(|option| {
                        option.category == Some(SessionConfigOptionCategory::Mode)
                            && select_contains(&option.kind, mode)
                    })
                })
    }

    /// 从 exact Session 当前 ACK 状态解析本次实际模型与推理强度；任何缺失或冲突都拒绝派发。
    pub(super) fn effective_execution_profile(&self) -> Result<ExecutionProfile, Failure> {
        self.validate()?;
        let options = self.response.config_options.as_deref().unwrap_or_default();
        let model_option =
            unique_select_current(options, "model", SessionConfigOptionCategory::Model)?;
        let models = self.models.as_ref().ok_or(Failure::Configuration)?;
        let current = models
            .get("currentModelId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or(Failure::Configuration)?;
        let model = models
            .get("availableModels")
            .and_then(Value::as_array)
            .and_then(|available| {
                available
                    .iter()
                    .find(|model| model["modelId"].as_str() == Some(current))
            })
            .ok_or(Failure::Configuration)?;
        let supports_reasoning = match model.get("_meta") {
            None => true,
            Some(Value::Object(metadata)) => match metadata.get("supportsReasoning") {
                Some(value) => value.as_bool().ok_or(Failure::Configuration)?,
                None => true,
            },
            Some(_) => return Err(Failure::Configuration),
        };
        if model_option
            .as_deref()
            .is_some_and(|option| option != current)
        {
            return Err(Failure::Configuration);
        }
        let reasoning = if supports_reasoning {
            let reasoning = unique_select_current(
                options,
                "thought_level",
                SessionConfigOptionCategory::ThoughtLevel,
            )?
            .ok_or(Failure::Configuration)?;
            Some(reasoning)
        } else {
            None
        };
        let profile = ExecutionProfile {
            model: Some(current.to_owned()),
            reasoning,
        };
        profile.validate().map_err(|_| Failure::Configuration)?;
        Ok(profile)
    }

    /// 只应用 exact session 的 early config update；其它原序帧保留给下一卡。
    pub(super) fn replay(
        &mut self,
        session_id: &str,
        frames: &[SessionFrame],
    ) -> Result<(), Failure> {
        for frame in frames {
            if frame.session_id != session_id {
                return Err(Failure::Malformed);
            }
            let update = &frame.params["update"];
            if update["sessionUpdate"] == "current_mode_update" {
                let mode = serde_json::from_value::<CurrentModeUpdate>(update.clone())
                    .map_err(|_| Failure::Malformed)?;
                let id = mode.current_mode_id.to_string();
                if !self.advertises_mode(&id) {
                    return Err(Failure::Configuration);
                }
                // typed 模式通知与 config ACK 一样同步两种目录；confirm 不得忽略撤销 auto。
                if let Some(modes) = &mut self.response.modes {
                    modes.current_mode_id = mode.current_mode_id;
                }
                if let Some(options) = &mut self.response.config_options {
                    for option in options {
                        if option.category == Some(SessionConfigOptionCategory::Mode)
                            && let SessionConfigKind::Select(select) = &mut option.kind
                        {
                            select.current_value = id.clone().into();
                        }
                    }
                }
            } else if update["sessionUpdate"] == "config_option_update" {
                let options = update
                    .get("configOptions")
                    .and_then(Value::as_array)
                    .ok_or(Failure::Malformed)?;
                // 按项解析，避免顶层 VecSkipError 静默丢弃 malformed option。
                let options = options
                    .iter()
                    .map(|v| {
                        serde_json::from_value::<SessionConfigOption>(v.clone())
                            .map_err(|_| Failure::Malformed)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                validate_options(&options)?;
                if let Some(modes) = &mut self.response.modes {
                    for option in &options {
                        if option.category == Some(SessionConfigOptionCategory::Mode) {
                            let SessionConfigKind::Select(select) = &option.kind else {
                                return Err(Failure::Configuration);
                            };
                            modes.current_mode_id = select.current_value.to_string().into();
                        }
                    }
                }
                self.response.config_options = Some(options);
            }
        }
        self.validate()
    }

    /// 目录身份必须自洽，不能由 option 数量或历史版本推断。
    fn validate(&self) -> Result<(), Failure> {
        if let Some(modes) = &self.response.modes {
            let mut ids = HashSet::new();
            for mode in &modes.available_modes {
                if mode.id.to_string().is_empty() || !ids.insert(mode.id.to_string()) {
                    return Err(Failure::Configuration);
                }
            }
            if !ids.contains(&modes.current_mode_id.to_string()) {
                return Err(Failure::Configuration);
            }
        }
        if let Some(options) = &self.response.config_options {
            validate_options(options)?;
            if let Some(modes) = &self.response.modes
                && options.iter().any(|option| {
                    option.category == Some(SessionConfigOptionCategory::Mode)
                        && !current_equals(
                            option,
                            &modes.current_mode_id.to_string().as_str().into(),
                        )
                })
            {
                return Err(Failure::Configuration);
            }
        }
        if let Some(models) = &self.models {
            let current = models
                .get("currentModelId")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .ok_or(Failure::Configuration)?;
            let available = models
                .get("availableModels")
                .and_then(Value::as_array)
                .ok_or(Failure::Configuration)?;
            let mut ids = HashSet::new();
            for model in available {
                let id = model
                    .get("modelId")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .ok_or(Failure::Configuration)?;
                if !ids.insert(id) {
                    return Err(Failure::Configuration);
                }
            }
            if !ids.contains(current) {
                return Err(Failure::Configuration);
            }
        }
        Ok(())
    }

    /// 本次显式配置复用 typed 请求，未指定权限模式时不覆盖 Provider 当前值。
    pub(super) async fn configure(
        &mut self,
        requests: &super::client::Requests,
        desired: &DesiredConfiguration,
    ) -> Result<(), Failure> {
        self.validate()?;
        let session_id = self.response.session_id.clone();
        if let Some(mode) = &desired.mode {
            if mode.is_empty() || !self.advertises_mode(mode) {
                return Err(Failure::Configuration);
            }
            if self
                .response
                .modes
                .as_ref()
                .is_some_and(|m| !m.available_modes.iter().any(|v| v.id.to_string() == *mode))
                || self
                    .response
                    .config_options
                    .as_ref()
                    .is_some_and(|options| {
                        options.iter().any(|option| {
                            option.category == Some(SessionConfigOptionCategory::Mode)
                                && !select_contains(&option.kind, mode)
                        })
                    })
            {
                return Err(Failure::Configuration);
            }
            requests
                .request(SetSessionModeRequest::new(session_id.clone(), mode.clone()))
                .await?;
            if let Some(modes) = &mut self.response.modes {
                modes.current_mode_id = mode.clone().into();
            }
            if let Some(options) = &mut self.response.config_options {
                for option in options {
                    if option.category == Some(SessionConfigOptionCategory::Mode)
                        && let SessionConfigKind::Select(select) = &mut option.kind
                    {
                        select.current_value = mode.clone().into();
                    }
                }
            }
        }
        if let Some(model) = &desired.model {
            // raw models 是可执行模型的 authority，不能仅凭 configOptions 猜测有效模型。
            let advertised = self.models.as_ref().is_some_and(|models| {
                models["availableModels"]
                    .as_array()
                    .is_some_and(|available| {
                        available
                            .iter()
                            .any(|candidate| candidate["modelId"].as_str() == Some(model))
                    })
            });
            if !advertised {
                return Err(Failure::Configuration);
            }
            self.apply_named_option(
                requests,
                &session_id,
                "model",
                SessionConfigOptionCategory::Model,
                model,
            )
            .await?;
            if let Some(models) = &mut self.models {
                models["currentModelId"] = Value::String(model.clone());
            }
        }
        // model ACK 返回的完整 options 是 reasoning 校验的新 authority。
        if let Some(reasoning) = &desired.reasoning {
            self.apply_named_option(
                requests,
                &session_id,
                "thought_level",
                SessionConfigOptionCategory::ThoughtLevel,
                reasoning,
            )
            .await?;
        }
        if let Some((id, value)) = &desired.option {
            let option = self
                .response
                .config_options
                .as_ref()
                .and_then(|options| options.iter().find(|option| option.id.to_string() == *id))
                .ok_or(Failure::Configuration)?;
            if !option_accepts(option, value) {
                return Err(Failure::Configuration);
            }
            let response = requests
                .request(SetSessionConfigOptionRequest::new(
                    session_id,
                    id.clone(),
                    value.clone(),
                ))
                .await?;
            validate_options(&response.config_options)?;
            if let Some(modes) = &mut self.response.modes {
                for option in &response.config_options {
                    if option.category == Some(SessionConfigOptionCategory::Mode) {
                        let SessionConfigKind::Select(select) = &option.kind else {
                            return Err(Failure::Configuration);
                        };
                        modes.current_mode_id = select.current_value.to_string().into();
                    }
                }
            }
            if !response
                .config_options
                .iter()
                .any(|option| option.id.to_string() == *id && current_equals(option, value))
            {
                return Err(Failure::Configuration);
            }
            self.response.config_options = Some(response.config_options);
        }
        Ok(())
    }

    /// 对 exact category/id 应用一个 select，并以 ACK 的完整目录替换旧 authority。
    async fn apply_named_option(
        &mut self,
        requests: &super::client::Requests,
        session_id: &SessionId,
        id: &str,
        category: SessionConfigOptionCategory,
        selected: &str,
    ) -> Result<(), Failure> {
        let value: SessionConfigOptionValue = selected.into();
        let option = self
            .response
            .config_options
            .as_ref()
            .and_then(|options| {
                options.iter().find(|option| {
                    option.id.to_string() == id && option.category == Some(category.clone())
                })
            })
            .ok_or(Failure::Configuration)?;
        if !option_accepts(option, &value) {
            return Err(Failure::Configuration);
        }
        let response = requests
            .request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                id.to_owned(),
                value.clone(),
            ))
            .await?;
        validate_options(&response.config_options)?;
        let actual = response.config_options.iter().find(|option| {
            option.id.to_string() == id && option.category == Some(category.clone())
        });
        if !actual.is_some_and(|option| current_equals(option, &value)) {
            return Err(Failure::Configuration);
        }
        self.response.config_options = Some(response.config_options);
        Ok(())
    }

    /// ACK 前已入队的更新不得撤销明确配置后仍产生 acceptance_ready。
    pub(super) fn confirm_desired(&self, desired: &DesiredConfiguration) -> Result<(), Failure> {
        self.validate()?;
        if let Some(mode) = &desired.mode
            && (!self.advertises_mode(mode)
                || self
                    .response
                    .modes
                    .as_ref()
                    .is_some_and(|m| m.current_mode_id.to_string() != *mode)
                || self
                    .response
                    .config_options
                    .as_ref()
                    .is_some_and(|options| {
                        options.iter().any(|option| {
                            option.category == Some(SessionConfigOptionCategory::Mode)
                                && !current_equals(option, &mode.as_str().into())
                        })
                    }))
        {
            return Err(Failure::Configuration);
        }
        for (selected, id, category) in [
            (&desired.model, "model", SessionConfigOptionCategory::Model),
            (
                &desired.reasoning,
                "thought_level",
                SessionConfigOptionCategory::ThoughtLevel,
            ),
        ] {
            if let Some(selected) = selected {
                let value: SessionConfigOptionValue = selected.as_str().into();
                if !self
                    .response
                    .config_options
                    .as_ref()
                    .is_some_and(|options| {
                        options.iter().any(|option| {
                            option.id.to_string() == id
                                && option.category == Some(category.clone())
                                && current_equals(option, &value)
                        })
                    })
                {
                    return Err(Failure::Configuration);
                }
            }
        }
        if let Some((id, value)) = &desired.option
            && !self
                .response
                .config_options
                .as_ref()
                .is_some_and(|options| {
                    options
                        .iter()
                        .any(|option| option.id.to_string() == *id && current_equals(option, value))
                })
        {
            return Err(Failure::Configuration);
        }
        Ok(())
    }

    /// 将 ACP Session 目录投影为 Product 统一模型，不暴露 `_meta` 原始对象。
    pub(crate) fn configuration_catalog(&self) -> Result<ExecutionConfigurationCatalog, Failure> {
        self.validate()?;
        let options = self.response.config_options.as_deref().unwrap_or_default();
        let model_option = options.iter().find(|option| {
            option.id.to_string() == "model"
                && option.category == Some(SessionConfigOptionCategory::Model)
        });
        let reasoning_option = options.iter().find(|option| {
            option.id.to_string() == "thought_level"
                && option.category == Some(SessionConfigOptionCategory::ThoughtLevel)
        });
        let reasoning_options = reasoning_option
            .map(project_select_options)
            .transpose()?
            .unwrap_or_default();
        let current_reasoning = reasoning_option.and_then(current_select_value);
        let raw_models = self.models.as_ref().ok_or(Failure::Configuration)?;
        let current_model = raw_models["currentModelId"].as_str().map(str::to_owned);
        let models = raw_models["availableModels"]
            .as_array()
            .ok_or(Failure::Configuration)?
            .iter()
            .map(|model| {
                let id = model["modelId"]
                    .as_str()
                    .ok_or(Failure::Configuration)?
                    .to_owned();
                let supports_reasoning = model
                    .get("_meta")
                    .and_then(|meta| meta.get("supportsReasoning"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                Ok(ExecutionModelOption {
                    name: model["name"].as_str().unwrap_or(&id).to_owned(),
                    description: model["description"].as_str().map(str::to_owned),
                    id,
                    is_default: false,
                    hidden: false,
                    reasoning_options: if supports_reasoning {
                        reasoning_options.clone()
                    } else {
                        Vec::new()
                    },
                    default_reasoning: None,
                })
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        Ok(ExecutionConfigurationCatalog {
            provider_id: ProviderId::new("codebuddy".into()).expect("static provider id is valid"),
            models,
            current_model: current_model.or_else(|| model_option.and_then(current_select_value)),
            default_model: None,
            reasoning_options,
            current_reasoning,
            default_reasoning: None,
        })
    }

    /// 临时 Provider catalog Session 逐模型读取真实 ACK，避免把当前模型的 thought_level
    /// 误投影给其它模型。Runtime 查询完成后立即销毁，因此无需切回原模型。
    pub(crate) async fn configuration_catalog_for_provider(
        &mut self,
        requests: &super::client::Requests,
    ) -> Result<ExecutionConfigurationCatalog, Failure> {
        let mut projected = self.configuration_catalog()?;
        let original_current = projected.current_model.clone();
        let session_id = self.response.session_id.clone();

        for index in 0..projected.models.len() {
            let model_id = projected.models[index].id.clone();
            let supports_reasoning = self
                .models
                .as_ref()
                .and_then(|models| models["availableModels"].as_array())
                .and_then(|models| {
                    models
                        .iter()
                        .find(|model| model["modelId"].as_str() == Some(model_id.as_str()))
                })
                .map(|model| {
                    model
                        .get("_meta")
                        .and_then(|meta| meta.get("supportsReasoning"))
                        .and_then(Value::as_bool)
                        .unwrap_or(true)
                })
                .ok_or(Failure::Configuration)?;

            if !supports_reasoning {
                projected.models[index].reasoning_options.clear();
                projected.models[index].default_reasoning = None;
                continue;
            }
            if original_current.as_deref() == Some(model_id.as_str()) {
                continue;
            }

            self.apply_named_option(
                requests,
                &session_id,
                "model",
                SessionConfigOptionCategory::Model,
                &model_id,
            )
            .await?;
            if let Some(models) = &mut self.models {
                models["currentModelId"] = Value::String(model_id.clone());
            }

            let reasoning = self
                .response
                .config_options
                .as_ref()
                .and_then(|options| {
                    options.iter().find(|option| {
                        option.id.to_string() == "thought_level"
                            && option.category == Some(SessionConfigOptionCategory::ThoughtLevel)
                    })
                })
                .ok_or(Failure::Configuration)?;
            projected.models[index].reasoning_options = project_select_options(reasoning)?;
            // ACP exposes the current session value but does not distinguish it from a durable
            // provider default, so do not infer default_reasoning here.
            projected.models[index].default_reasoning = None;
        }

        Ok(projected)
    }
}

/// 只投影 select 的稳定展示字段；group label 不进入 Product wire。
fn project_select_options(
    option: &SessionConfigOption,
) -> Result<Vec<ExecutionConfigurationOption>, Failure> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return Err(Failure::Configuration);
    };
    let entries: Vec<_> = match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .collect(),
        _ => return Err(Failure::Configuration),
    };
    Ok(entries
        .into_iter()
        .map(|entry| ExecutionConfigurationOption {
            id: entry.value.to_string(),
            name: entry.name.clone(),
            description: entry.description.clone(),
        })
        .collect())
}

/// 读取 select 当前值；非 select 不作为模型或推理 authority。
fn current_select_value(option: &SessionConfigOption) -> Option<String> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    Some(select.current_value.to_string())
}

/// exact id/category 只能出现一次；非 select、空值和重复 authority 均不可信。
fn unique_select_current(
    options: &[SessionConfigOption],
    id: &str,
    category: SessionConfigOptionCategory,
) -> Result<Option<String>, Failure> {
    let mut matching = options
        .iter()
        .filter(|option| option.id.to_string() == id && option.category == Some(category.clone()));
    let Some(option) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Err(Failure::Configuration);
    }
    current_select_value(option)
        .filter(|value| !value.is_empty())
        .map(Some)
        .ok_or(Failure::Configuration)
}

/// 保留 SDK typed select/group 身份；未知类型不能作为配置授权。
fn select_contains(kind: &SessionConfigKind, value: &str) -> bool {
    let SessionConfigKind::Select(select) = kind else {
        return false;
    };
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .any(|option| option.value.to_string() == value),
        SessionConfigSelectOptions::Grouped(groups) => groups.iter().any(|group| {
            group
                .options
                .iter()
                .any(|option| option.value.to_string() == value)
        }),
        _ => false,
    }
}

/// 当前值必须存在于目录，重复 id 不得产生模糊目标。
fn validate_options(options: &[SessionConfigOption]) -> Result<(), Failure> {
    let mut ids = HashSet::new();
    for option in options {
        if option.id.to_string().is_empty() || !ids.insert(option.id.to_string()) {
            return Err(Failure::Configuration);
        }
        match &option.kind {
            SessionConfigKind::Select(select)
                if select_contains(&option.kind, &select.current_value.to_string()) =>
            {
                let values: Vec<String> = match &select.options {
                    SessionConfigSelectOptions::Ungrouped(options) => {
                        options.iter().map(|v| v.value.to_string()).collect()
                    }
                    SessionConfigSelectOptions::Grouped(groups) => groups
                        .iter()
                        .flat_map(|g| g.options.iter().map(|v| v.value.to_string()))
                        .collect(),
                    _ => return Err(Failure::Configuration),
                };
                let mut unique = HashSet::new();
                if values.iter().any(|v| v.is_empty() || !unique.insert(v)) {
                    return Err(Failure::Configuration);
                }
            }
            SessionConfigKind::Boolean(_) => {}
            _ => return Err(Failure::Configuration),
        }
    }
    Ok(())
}

/// boolean 必须是实际 boolean option；multitask=true 永远不能由本卡发送。
fn option_accepts(option: &SessionConfigOption, value: &SessionConfigOptionValue) -> bool {
    match (&option.kind, value) {
        (SessionConfigKind::Select(_), SessionConfigOptionValue::ValueId { value }) => {
            !(option.id.to_string() == "multitask" && value.to_string() == "true")
                && select_contains(&option.kind, &value.to_string())
        }
        (SessionConfigKind::Boolean(_), SessionConfigOptionValue::Boolean { value }) => {
            !(option.id.to_string() == "multitask" && *value)
        }
        _ => false,
    }
}

/// typed config ACK 应确认请求值，不能把另一个当前值当成配置完成。
fn current_equals(option: &SessionConfigOption, value: &SessionConfigOptionValue) -> bool {
    match (&option.kind, value) {
        (SessionConfigKind::Select(select), SessionConfigOptionValue::ValueId { value }) => {
            select.current_value == *value
        }
        (SessionConfigKind::Boolean(current), SessionConfigOptionValue::Boolean { value }) => {
            current.current_value == *value
        }
        _ => false,
    }
}

#[cfg(all(test, windows))]
pub(super) mod tests;

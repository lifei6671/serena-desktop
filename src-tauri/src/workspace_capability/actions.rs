//! Local Human capability action 的 single-flight、独立活动与临时 ownership。
use super::*;
use tokio_util::sync::CancellationToken;

/// 显式操作只允许有限执行时间；超时后丢弃 Provider future 并释放 operation claim。
const ACTION_TIMEOUT: Duration = Duration::from_secs(120);

/// 本地调用返回本次操作身份；readiness 不形成持久 Index Health authority。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityActionResult {
    pub(crate) operation_id: String,
    pub(crate) readiness: CapabilityReadinessState,
}

/// 同一四元组的所有调用观察同一个 completion，不另起 Provider action。
pub(super) struct ActionFlight {
    pub(super) operation_id: String,
    pub(super) exclusive: bool,
    root: PathBuf,
    cancellation: CancellationToken,
    // 所有读写均持有 runtime_slots 锁；atomic 仅用于 Arc 内部可变性，不建立第二个同步边界。
    accepting_cancellation: std::sync::atomic::AtomicBool,
    pub(super) completion:
        watch::Sender<Option<Result<CapabilityActionResult, WorkspaceCapabilityError>>>,
}

/// Operation 自有活动投影；不接触 Agent Execution/Activity/Control 的任何 revision。
struct Operation {
    table: Arc<Mutex<RuntimeSlotTable>>,
    key: (RuntimeSlotKey, String),
    flight: Arc<ActionFlight>,
    sink: Arc<dyn CapabilityActivitySink>,
    activity: CapabilityActivity,
    finished: bool,
}

impl Operation {
    /// 只发布 Manager 生成的封闭状态；Provider 人类文本不会进入事件。
    async fn publish(&self) {
        self.sink.publish(self.activity.clone()).await;
    }

    /// 先发布 terminal 再交付同一个结果，并收敛临时 claim。
    async fn complete(mut self, result: Result<CapabilityActionResult, WorkspaceCapabilityError>) {
        let result = {
            let _table = lock_unpoisoned(&self.table);
            // cancel 已先在线性化边界内返回 Ok 时，必须覆盖 select 的 candidate result。
            let result = if self.flight.cancellation.is_cancelled() {
                Err(prepare_failed())
            } else {
                result
            };
            self.flight
                .accepting_cancellation
                .store(false, std::sync::atomic::Ordering::Relaxed);
            result
        };
        self.activity.revision = 2;
        self.activity.state = if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        };
        self.activity.message_code = if result.is_ok() {
            "CAPABILITY_ACTION_SUCCEEDED"
        } else {
            "CAPABILITY_ACTION_FAILED"
        };
        self.publish().await;
        let mut table = lock_unpoisoned(&self.table);
        self.flight.completion.send_replace(Some(result));
        table.operations.remove(&self.key);
        self.finished = true;
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // 任务取消或 panic 也不能留下永久 busy；Provider future 先于本 guard 析构。
        self.flight
            .completion
            .send_replace(Some(Err(prepare_failed())));
        lock_unpoisoned(&self.table).operations.remove(&self.key);
        let mut activity = self.activity.clone();
        activity.revision = 2;
        activity.state = "failed";
        activity.message_code = "CAPABILITY_ACTION_FAILED";
        let sink = Arc::clone(&self.sink);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move { sink.publish(activity).await });
        }
    }
}

/// Provider prepare 的失败只返回统一安全分类。
fn prepare_failed() -> WorkspaceCapabilityError {
    WorkspaceCapabilityError {
        code: WorkspaceCapabilityErrorCode::PrepareFailed,
    }
}

impl WorkspaceCapabilityManager {
    /// 仅取消当前进程中 Manager 创建的 operation；重复触发同一 running flight 是幂等的。
    pub(crate) fn cancel_action(&self, operation_id: &str) -> Result<(), WorkspaceCapabilityError> {
        let table = lock_unpoisoned(&self.runtime_slots);
        let flight = table
            .operations
            .values()
            .find(|flight| flight.operation_id == operation_id)
            .ok_or_else(WorkspaceCapabilityError::not_found)?;
        if !flight
            .accepting_cancellation
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err(WorkspaceCapabilityError::not_found());
        }
        // 查找和触发位于同一 admission 边界；不创建完成记录或第二份 operation 索引。
        flight.cancellation.cancel();
        Ok(())
    }

    /// 仅由已解析 Lease 的本地入口调用；Descriptor 是唯一 Provider/Action authority。
    pub(crate) async fn prepare_action(
        self: &Arc<Self>,
        lease: WorkspaceLease,
        provider_id: &str,
        action_id: &str,
        sink: Arc<dyn CapabilityActivitySink>,
    ) -> Result<CapabilityActionResult, WorkspaceCapabilityError> {
        let provider = self.provider(provider_id)?;
        let descriptor = provider.descriptor();
        let action = descriptor
            .action_descriptors
            .iter()
            .find(|action| action.action_id == action_id)
            .ok_or_else(WorkspaceCapabilityError::not_found)?
            .clone();
        if action.authority != CapabilityActionAuthority::LocalHuman {
            return Err(WorkspaceCapabilityError::contract_error());
        }
        let key = (
            RuntimeSlotKey::new(descriptor.provider_id.clone(), &lease),
            action.action_id.clone(),
        );
        let (flight, operation) = {
            let mut table = lock_unpoisoned(&self.runtime_slots);
            if table.shutting_down
                || table
                    .removing_workspaces
                    .contains(&(lease.workspace_id.clone(), lease.generation))
            {
                return Err(WorkspaceCapabilityError::busy());
            }
            if let Some(flight) = table.operations.get(&key) {
                if flight.root != lease.canonical_root {
                    return Err(WorkspaceCapabilityError::contract_error());
                }
                (Arc::clone(flight), None)
            } else {
                // 不同 action 不能绕过同 Slot operation；相同 action 已在上面 join。
                if table.operations.keys().any(|(slot, _)| slot == &key.0) {
                    return Err(WorkspaceCapabilityError::busy());
                }
                if let Some(slot) = table.slots.get(&key.0) {
                    if slot.canonical_root != lease.canonical_root {
                        return Err(WorkspaceCapabilityError::contract_error());
                    }
                    let state = lock_unpoisoned(&slot.state);
                    if action.execution == CapabilityActionExecution::ProviderPrepare
                        && (state.in_flight != 0
                            || state.pending_acquires != 0
                            || matches!(
                                state.lifecycle,
                                CapabilityRuntimeState::Starting | CapabilityRuntimeState::Stopping
                            ))
                    {
                        return Err(WorkspaceCapabilityError::busy());
                    }
                }
                let mut random = [0u8; 16];
                getrandom::fill(&mut random).map_err(|_| prepare_failed())?;
                let operation_id = format!(
                    "cap-op-{}",
                    random
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                );
                let flight = Arc::new(ActionFlight {
                    operation_id: operation_id.clone(),
                    exclusive: action.execution == CapabilityActionExecution::ProviderPrepare,
                    root: lease.canonical_root.clone(),
                    cancellation: CancellationToken::new(),
                    accepting_cancellation: std::sync::atomic::AtomicBool::new(true),
                    completion: watch::channel(None).0,
                });
                table.operations.insert(key.clone(), Arc::clone(&flight));
                let operation = Operation {
                    table: Arc::clone(&self.runtime_slots),
                    key,
                    flight: Arc::clone(&flight),
                    sink,
                    activity: CapabilityActivity {
                        operation_id,
                        workspace_id: lease.workspace_id.clone(),
                        provider_id: descriptor.provider_id.clone(),
                        action_id: action.action_id.clone(),
                        stage_code: match action.execution {
                            CapabilityActionExecution::ManagerEnsureRuntime => "starting_runtime",
                            CapabilityActionExecution::ProviderPrepare => "preparing",
                        },
                        state: "running",
                        revision: 1,
                        message_code: "CAPABILITY_ACTION_RUNNING",
                    },
                    finished: false,
                };
                (flight, Some(operation))
            }
        };
        if let Some(operation) = operation {
            let manager = Arc::clone(self);
            // Caller Drop 不取消其他 joiner；只有显式 operation cancel 或 timeout 结束共享执行。
            tokio::spawn(async move {
                operation.publish().await;
                let result = tokio::select! {
                    // 已收到取消时优先收敛；select 结束会先 drop execute future，再发布 terminal。
                    biased;
                    _ = operation.flight.cancellation.cancelled() => Err(prepare_failed()),
                    result = tokio::time::timeout(
                        ACTION_TIMEOUT,
                        manager.execute_action(&lease, provider, &action, &operation),
                    ) => result.unwrap_or_else(|_| Err(prepare_failed())),
                };
                operation.complete(result).await;
            });
        }
        let mut completion = flight.completion.subscribe();
        loop {
            if let Some(result) = completion.borrow_and_update().clone() {
                return result;
            }
            completion.changed().await.map_err(|_| prepare_failed())?;
        }
    }

    /// 以 Descriptor execution 分派，绝不按具体 Provider ID 分支。
    async fn execute_action(
        &self,
        lease: &WorkspaceLease,
        provider: Arc<dyn WorkspaceCapabilityProvider>,
        action: &CapabilityActionDescriptor,
        operation: &Operation,
    ) -> Result<CapabilityActionResult, WorkspaceCapabilityError> {
        let id = provider.descriptor().provider_id.as_str();
        let operation_id = operation.flight.operation_id.as_str();
        let readiness = match action.execution {
            CapabilityActionExecution::ManagerEnsureRuntime => {
                drop(
                    self.acquire_action_runtime(id, lease.clone(), Some(operation_id))
                        .await?,
                );
                if !action.warm_runtime {
                    self.stop_action_slot(&operation.key.0).await?;
                }
                CapabilityReadinessState::Ready
            }
            CapabilityActionExecution::ProviderPrepare => {
                self.stop_action_slot(&operation.key.0).await?;
                let result = provider
                    .prepare(
                        lease.clone(),
                        CapabilityPrepareAction {
                            action_id: action.action_id.clone(),
                        },
                        &SafeProviderSink,
                    )
                    .await
                    .map_err(|error| match error.code {
                        CapabilityProviderErrorCode::ContractError
                        | CapabilityProviderErrorCode::RuntimeIdentityMismatch => {
                            WorkspaceCapabilityError::contract_error()
                        }
                        _ => prepare_failed(),
                    })?;
                if action.warm_runtime {
                    drop(
                        self.acquire_action_runtime(id, lease.clone(), Some(operation_id))
                            .await?,
                    );
                }
                result.readiness
            }
        };
        Ok(CapabilityActionResult {
            operation_id: operation_id.to_owned(),
            readiness,
        })
    }

    /// exclusive claim 内复用既有 stop single-flight，失败时保留原 handle/capacity。
    async fn stop_action_slot(&self, key: &RuntimeSlotKey) -> Result<(), WorkspaceCapabilityError> {
        let exists = lock_unpoisoned(&self.runtime_slots).slots.contains_key(key);
        if exists {
            self.drain_workspace_remove_slot(key).await
        } else {
            Ok(())
        }
    }
}

/// 本步仅发布 Manager 的 running/terminal；不信任 Provider 传入的自由文本或 identity。
struct SafeProviderSink;
impl CapabilityActivitySink for SafeProviderSink {}

//! Execution-local bindings are reset only with an exclusive pool lease.
use super::*;
use std::sync::atomic::AtomicUsize;

pub(super) struct ServerWork(pub Arc<AtomicUsize>);
impl Drop for ServerWork {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Shared {
    pub(super) fn completed_notification(&self, event: &Notification) -> bool {
        let identity = match event {
            Notification::TurnStarted { thread_id, turn }
            | Notification::TurnCompleted { thread_id, turn } => (thread_id, &turn.id),
            Notification::TurnError {
                thread_id, turn_id, ..
            }
            | Notification::PermissionDenied {
                thread_id, turn_id, ..
            } => (thread_id, turn_id),
            Notification::Activity(a) => (&a.thread_id, &a.turn_id),
            _ => return false,
        };
        self.completed_turns
            .lock()
            .unwrap()
            .contains(&(identity.0.clone(), identity.1.clone()))
    }
}

impl Client {
    pub(crate) fn loaded_thread(&self, id: &str) -> Option<Thread> {
        self.shared.loaded_threads.lock().unwrap().get(id).cloned()
    }

    pub(crate) fn reusable(&self) -> bool {
        self.is_ready()
            && self.shared.pending.lock().unwrap().is_empty()
            && self.shared.server_work.load(Ordering::Acquire) == 0
    }

    pub(crate) async fn prepare_execution(
        &self,
        store: &crate::agent::store::StateStore,
    ) -> Result<()> {
        if !self.reusable()
            || self.shared.title_scope.lock().unwrap().is_some()
            || self.shared.observability_scope.lock().unwrap().is_some()
        {
            return Err(ProtocolError::invalid(
                "Client is not available for an Execution",
            ));
        }
        self.drain_completed_events(store).await
    }

    pub(crate) async fn finish_execution(
        &self,
        store: &crate::agent::store::StateStore,
        thread: Option<&str>,
        turn: Option<&str>,
    ) -> Result<()> {
        if !self.reusable() {
            return Err(ProtocolError::invalid(
                "Client has unresolved work or transport failure",
            ));
        }
        let (Some(thread), Some(turn)) = (thread, turn) else {
            return Err(ProtocolError::invalid(
                "Completed Execution identity required",
            ));
        };
        self.shared
            .completed_turns
            .lock()
            .unwrap()
            .insert((thread.into(), turn.into()));
        *self.shared.title_scope.lock().unwrap() = None;
        *self.shared.observability_scope.lock().unwrap() = None;
        // Consume the previous activity watch version without sending a None event.
        self.activity.lock().await.borrow_and_update();
        self.drain_completed_events(store).await?;
        if !self.reusable() {
            return Err(ProtocolError::invalid("Client became unavailable"));
        }
        Ok(())
    }

    async fn drain_completed_events(&self, store: &crate::agent::store::StateStore) -> Result<()> {
        let mut events = self.events.lock().await;
        while let Ok(event) = events.try_recv() {
            if self.shared.completed_notification(&event.notification) {
                continue;
            }
            match event.notification {
                Notification::ThreadStarted(_) | Notification::SubAgentStarted { .. } => {}
                Notification::ThreadNameUpdated { thread_id, name } => {
                    let owned = {
                        let mut threads = self.shared.loaded_threads.lock().unwrap();
                        if let Some(thread) = threads.get_mut(&thread_id) {
                            thread.name = name.clone();
                            true
                        } else {
                            false
                        }
                    };
                    if owned {
                        store
                            .save_thread_name(thread_id, name)
                            .await
                            .map_err(|e| ProtocolError::new("CODEX_EXECUTION_STORE_FAILED", e))?;
                    }
                }
                _ => {
                    return Err(ProtocolError::invalid(
                        "Unresolved lifecycle event at Execution boundary",
                    ));
                }
            }
        }
        Ok(())
    }
}

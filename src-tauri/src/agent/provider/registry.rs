use rmcp::schemars;
use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
};

use super::{
    ProviderCapabilities, ProviderDescriptor, ProviderError, ProviderErrorCode, ProviderId,
    port::AgentProvider,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealth {
    Available,
    Unavailable,
}

#[derive(Clone)]
struct RegistryEntry {
    provider: Arc<dyn AgentProvider>,
    health: ProviderHealth,
}

#[derive(Clone, Default)]
pub struct ProviderRegistry {
    entries: HashMap<ProviderId, RegistryEntry>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        provider: Arc<dyn AgentProvider>,
        health: ProviderHealth,
    ) -> Result<(), ProviderError> {
        let id = provider.descriptor().id;
        match self.entries.entry(id) {
            Entry::Vacant(entry) => {
                entry.insert(RegistryEntry { provider, health });
                Ok(())
            }
            Entry::Occupied(_) => Err(ProviderError {
                code: ProviderErrorCode::AgentProviderContractError,
            }),
        }
    }

    pub fn get(&self, id: &ProviderId) -> Result<Arc<dyn AgentProvider>, ProviderError> {
        let entry = self.entries.get(id).ok_or(ProviderError {
            code: ProviderErrorCode::AgentProviderNotFound,
        })?;
        if self.health(id)? == ProviderHealth::Unavailable {
            return Err(ProviderError {
                code: ProviderErrorCode::AgentProviderUnavailable,
            });
        }

        Ok(entry.provider.clone())
    }

    pub fn get_registered(&self, id: &ProviderId) -> Result<Arc<dyn AgentProvider>, ProviderError> {
        self.entries
            .get(id)
            .map(|entry| entry.provider.clone())
            .ok_or(ProviderError {
                code: ProviderErrorCode::AgentProviderNotFound,
            })
    }

    pub(crate) fn set_health(
        &mut self,
        id: &ProviderId,
        health: ProviderHealth,
    ) -> Result<(), ProviderError> {
        let entry = self.entries.get_mut(id).ok_or(ProviderError {
            code: ProviderErrorCode::AgentProviderNotFound,
        })?;
        entry.health = health;
        Ok(())
    }

    /// 只替换已注册的 admission adapter，旧调用持有的 Arc 不受影响。
    pub(crate) fn replace_registered(
        &mut self,
        provider: Arc<dyn AgentProvider>,
        health: ProviderHealth,
    ) -> Result<(), ProviderError> {
        let id = provider.descriptor().id;
        self.get_registered(&id)?;
        self.entries.insert(id, RegistryEntry { provider, health });
        Ok(())
    }

    pub fn list_descriptors(&self) -> Vec<ProviderDescriptor> {
        let mut descriptors: Vec<_> = self
            .entries
            .values()
            .map(|entry| entry.provider.descriptor())
            .collect();
        descriptors.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        descriptors
    }

    pub fn capabilities(&self, id: &ProviderId) -> Result<ProviderCapabilities, ProviderError> {
        let entry = self.entries.get(id).ok_or(ProviderError {
            code: ProviderErrorCode::AgentProviderNotFound,
        })?;
        Ok(entry.provider.capabilities())
    }

    pub fn health(&self, id: &ProviderId) -> Result<ProviderHealth, ProviderError> {
        self.admission_status(id).map(|(health, _)| health)
    }

    /// 返回 Provider 自己声明的确定性 admission 诊断，不推断 provider 身份或错误文本。
    pub fn diagnostic_code(&self, id: &ProviderId) -> Result<Option<String>, ProviderError> {
        self.admission_status(id).map(|(_, diagnostic)| diagnostic)
    }

    /// 单次读取 provider diagnostic，形成内部一致的 effective health 与诊断快照。
    pub(crate) fn admission_status(
        &self,
        id: &ProviderId,
    ) -> Result<(ProviderHealth, Option<String>), ProviderError> {
        self.entries
            .get(id)
            .map(|entry| {
                let diagnostic = entry.provider.admission_diagnostic();
                // admission diagnostic 只覆盖新执行的有效健康；stored discovery health 保持不变。
                let health = if diagnostic.is_some() {
                    ProviderHealth::Unavailable
                } else {
                    entry.health
                };
                (health, diagnostic)
            })
            .ok_or(ProviderError {
                code: ProviderErrorCode::AgentProviderNotFound,
            })
    }
}

#[cfg(test)]
mod tests;

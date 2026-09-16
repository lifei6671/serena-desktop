use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
};

use super::{
    ProviderCapabilities, ProviderDescriptor, ProviderError, ProviderErrorCode, ProviderId,
    port::AgentProvider,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderHealth {
    Available,
    Unavailable,
}

struct RegistryEntry {
    provider: Arc<dyn AgentProvider>,
    health: ProviderHealth,
}

#[derive(Default)]
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
        if entry.health == ProviderHealth::Unavailable {
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
        self.entries
            .get(id)
            .map(|entry| entry.health)
            .ok_or(ProviderError {
                code: ProviderErrorCode::AgentProviderNotFound,
            })
    }
}

#[cfg(test)]
mod tests;

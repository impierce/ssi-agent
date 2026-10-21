//! The port through which the KMS talks to key providers, and the in-memory provider.

use crate::key_provider::aggregate::KeyProviderType;
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

/// Why a key provider could not be reached. The message comes from the provider and is shown to the operator.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct ConnectionError(pub String);

/// A key that exists in a key provider, as reported by the provider.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ProviderKey {
    /// The provider's identifier for the key.
    pub id: String,
    /// The provider's full resource name for the key, where it has one (an ARN for AWS KMS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_name: Option<String>,
}

/// A connection to an external key provider. Key operations will be added later.
#[async_trait]
pub trait KeyProviderAdapter: Send + Sync {
    /// Checks that the provider can be reached with the configured credentials.
    async fn test_connection(&self) -> Result<(), ConnectionError>;

    /// Lists the keys that exist in the provider and that the configured credentials can see.
    async fn list_keys(&self) -> Result<Vec<ProviderKey>, ConnectionError>;
}

/// The adapter for each provider type this instance can use.
pub type KeyProviderAdapters = HashMap<KeyProviderType, Arc<dyn KeyProviderAdapter>>;

/// Keeps keys in this process. Always reachable.
pub struct InMemoryKeyProvider;

#[async_trait]
impl KeyProviderAdapter for InMemoryKeyProvider {
    async fn test_connection(&self) -> Result<(), ConnectionError> {
        Ok(())
    }

    async fn list_keys(&self) -> Result<Vec<ProviderKey>, ConnectionError> {
        Ok(Vec::new())
    }
}

//! The port through which the KMS talks to key providers, and the in-memory provider.

use crate::key_provider::aggregate::KeyProviderType;
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

/// Why a key provider could not be reached. The message comes from the provider and is shown to the operator.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct ConnectionError(pub String);

/// A connection to an external key provider. Only reachability for now; key operations will be added later.
#[async_trait]
pub trait KeyProviderAdapter: Send + Sync {
    /// Checks that the provider can be reached with the configured credentials.
    async fn test_connection(&self) -> Result<(), ConnectionError>;
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
}

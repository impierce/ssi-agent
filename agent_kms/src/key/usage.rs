//! What the KMS asks before it deletes a key: whether something still signs with it.

use async_trait::async_trait;
use cqrs_es::persist::PersistenceError;

/// Tells the KMS what signs with its keys, so that a key is never deleted while something still signs with it.
#[async_trait]
pub trait KeyUsage: Send + Sync {
    /// What signs with the key the KMS names `id`, such as the verification method it backs, or `None` when nothing
    /// does.
    ///
    /// # Errors
    ///
    /// Only when what uses keys can't be loaded.
    async fn user(&self, id: &str) -> Result<Option<String>, PersistenceError>;
}

/// Nothing signs with any key, for testing the KMS on its own.
#[cfg(any(test, feature = "test_utils"))]
pub struct Unused;

#[cfg(any(test, feature = "test_utils"))]
#[async_trait]
impl KeyUsage for Unused {
    async fn user(&self, _id: &str) -> Result<Option<String>, PersistenceError> {
        Ok(None)
    }
}

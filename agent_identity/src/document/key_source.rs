//! Where the keys added to the `did:web` document come from, such as the Key Management System.

use async_trait::async_trait;
use identity_kit::keys::{KeySigner, KeyStore};
use std::sync::Arc;
use thiserror::Error;

/// Gives the keys that verification methods added to the `did:web` document sign with. Asked once per added key at
/// startup, when a key is added, and again only for a key that isn't available, until it is.
#[async_trait]
pub trait KeySource: Send + Sync {
    /// The key `id` names, to sign with. Its signatures are only checked when they are made: a key that is there but
    /// can't sign, such as one disabled in its provider, may still be given.
    ///
    /// # Errors
    ///
    /// Fails when there is no such key, it can't sign as it is, or it can't be had now (see [`KeySourceError`]).
    async fn key(&self, id: &str) -> Result<KeySigner<Arc<dyn KeyStore>>, KeySourceError>;
}

/// Why a [`KeySource`] can't give a key. The variants tell whether asking again can help.
#[derive(Debug, Error)]
pub enum KeySourceError {
    /// There is no key with this id, or not anymore, such as one deleted from its provider. Asking again doesn't help.
    #[error("There is no key `{0}`")]
    NotFound(String),
    /// The key is there but can't sign as it is, such as one whose provider this instance can't reach at all. Asking
    /// again doesn't help.
    #[error("The key `{id}` can't sign: {reason}")]
    Unusable { id: String, reason: String },
    /// The key can't be had now, such as when its provider doesn't answer. Asking again later may give it.
    #[error("The key `{id}` is not available now: {reason}")]
    Unavailable { id: String, reason: String },
}

/// The source without keys, for an instance without the Key Management System: every key is [`KeySourceError::NotFound`].
pub struct NoKeys;

#[async_trait]
impl KeySource for NoKeys {
    async fn key(&self, id: &str) -> Result<KeySigner<Arc<dyn KeyStore>>, KeySourceError> {
        Err(KeySourceError::NotFound(id.to_string()))
    }
}

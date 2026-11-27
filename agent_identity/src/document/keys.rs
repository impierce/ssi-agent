//! The keys of the `did:web` document: adding keys of a [`KeySource`], such as the Key Management System, to it as
//! verification methods, selecting the verification method that signs per algorithm, and keeping the [`Subject`]'s
//! keys and signing cache in step with the documents.

use crate::document::{
    aggregate::{key_algorithm, Document, Status},
    command::{DocumentCommand, DocumentOperation},
    error::DocumentError,
    key_source::{KeySource, KeySourceError},
};
use crate::state::IdentityState;
use agent_secret_manager::subject::{MethodKey, StorageKey, Subject};
use agent_shared::config::SupportedDidMethod;
use agent_shared::handlers::{
    authorize_command, command_handler, public_query_handler, query_handler, CommandHandlerError, QueryHandlerError,
};
use cqrs_es::persist::PersistenceError;
use identity_kit::did::{Did, DidUrl, VerificationMethod};
use identity_kit::jose::Signer as _;
use indexmap::IndexMap;
use itertools::Itertools;
use jsonwebtoken::Algorithm;
use shared_kernel::authorization::{AuthorizationError, Caller};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::warn;

/// How long [`spawn_key_retry`] waits before it retries the unavailable keys a second time. Every further wait is
/// twice as long, up to [`MAX_RETRY_DELAY`].
const FIRST_RETRY_DELAY: Duration = Duration::from_secs(1);

/// The longest [`spawn_key_retry`] waits between two retries.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(300);

/// What a retried key signs, to tell a key that is there but can't sign, such as one disabled in its provider.
const PROBE: &[u8] = b"UniCore signing key probe";

/// Why an added verification method whose key isn't loaded can't sign.
const NOT_LOADED: &str = "its key isn't loaded yet";
/// Why a key could not be added to the `did:web` document.
#[derive(Debug, Error)]
pub enum AddKeyError {
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("There is no enabled `did:web` document to add the key to")]
    NoDidWebDocument,
    /// The key source can't give the key to sign with.
    #[error(transparent)]
    Key(#[from] KeySourceError),
    /// The key can't become a verification method of the document.
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error(transparent)]
    Command(#[from] CommandHandlerError<DocumentError>),
}

/// Why a key of the `did:web` document could not be removed, or its algorithm deselected.
#[derive(Debug, Error)]
pub enum DidWebDocumentError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("There is no enabled `did:web` document")]
    NoDidWebDocument,
    #[error(transparent)]
    Command(#[from] CommandHandlerError<DocumentError>),
}

/// Why a verification method could not be selected to sign with.
#[derive(Debug, Error)]
pub enum SelectSigningKeyError {
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    /// None of the documents has the DID the verification method belongs to.
    #[error("No DID document has the DID `{0}`")]
    UnknownDid(Did),
    /// The key of the verification method can't sign now.
    #[error("The key of `{method}` is not available to sign with: {reason}")]
    NotAvailable { method: DidUrl, reason: String },
    #[error(transparent)]
    Command(#[from] CommandHandlerError<DocumentError>),
}

/// The key an algorithm of the `did:web` document signs with, as this instance last saw it. Keys aren't checked in
/// their provider until they sign, so a key deleted or disabled there still shows as signing, without a `problem`,
/// until the next signature with it fails.
#[derive(Debug, Clone, PartialEq)]
pub struct SigningKey {
    /// The algorithm the key signs for.
    pub algorithm: Algorithm,
    /// The verification method that signs now.
    pub verification_method_id: DidUrl,
    /// The verification method selected to sign. When it differs from `verification_method_id`, it can't sign, and
    /// the configured issuer key signs instead.
    pub selected_verification_method_id: Option<DidUrl>,
    /// Why the selected method, or else the signing method, can't sign: its key wasn't loaded, failed the last time
    /// it signed, or is gone. `None` says only that no signature with it has failed yet.
    pub problem: Option<String>,
}

/// Adds the key `keys` names `key_id` to the `did:web` document, as a verification method named by the thumbprint of
/// its public key, and returns that method. The key can sign for the method right away, but only
/// signs for the document once the method is selected with [`select_signing_key`]. Adding a key again changes
/// nothing.
///
/// # Errors
///
/// Fails when there is no enabled `did:web` document, the caller isn't authorized, or `keys` can't give the key to sign
/// with, such as when its provider can't be reached.
pub async fn add_key_to_document(
    state: &IdentityState,
    keys: &dyn KeySource,
    caller: Caller,
    key_id: &str,
) -> Result<DidUrl, AddKeyError> {
    let Some(document) = did_web_document(state).await? else {
        return Err(AddKeyError::NoDidWebDocument);
    };
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        &document.document_id,
        &DocumentOperation::AddPublicKey,
    )
    .await
    .map_err(AddKeyError::Authorization)?;

    let signer = keys.key(key_id).await?;
    let public_key = signer.public_jwk().clone().with_alg(signer.alg().as_str());
    let method = document.added_verification_method(public_key.clone())?.id;

    let _guard = state.document_keys_lock.lock().await;
    command_handler(
        state.authorization_checker.clone(),
        caller,
        &document.document_id,
        &state.command.document,
        DocumentCommand::AddPublicKey {
            public_key: Box::new(public_key),
            key_id: key_id.to_string(),
        },
    )
    .await?;
    state
        .services
        .subject
        .set_method_key(method.clone(), MethodKey::Available(Box::new(signer)))
        .await;
    synchronize_subject(state).await?;
    Ok(method)
}

/// Removes the verification method of a key added with [`add_key_to_document`] from the `did:web` document. Nothing
/// signs for it anymore. The selected signing key of an algorithm isn't removed.
///
/// # Errors
///
/// Fails when there is no enabled `did:web` document, the caller isn't authorized, or the method isn't an added key
/// or is the selected signing key of its algorithm.
pub async fn remove_key_from_document(
    state: &IdentityState,
    caller: Caller,
    method: &DidUrl,
) -> Result<(), DidWebDocumentError> {
    change_did_web_document(
        state,
        caller,
        DocumentCommand::RemovePublicKey {
            verification_method_id: method.clone(),
        },
    )
    .await
}

/// Signs with the configured issuer key of `algorithm` again, in the `did:web` document. Without a selected key,
/// nothing changes.
///
/// # Errors
///
/// Fails when there is no enabled `did:web` document or the caller isn't authorized.
pub async fn deselect_signing_key(
    state: &IdentityState,
    caller: Caller,
    algorithm: Algorithm,
) -> Result<(), DidWebDocumentError> {
    change_did_web_document(state, caller, DocumentCommand::DeselectSigningKey { algorithm }).await
}

/// Executes `command` on the `did:web` document and synchronizes the subject with it.
async fn change_did_web_document(
    state: &IdentityState,
    caller: Caller,
    command: DocumentCommand,
) -> Result<(), DidWebDocumentError> {
    let Some(document) = did_web_document(state).await? else {
        return Err(DidWebDocumentError::NoDidWebDocument);
    };

    let _guard = state.document_keys_lock.lock().await;
    command_handler(
        state.authorization_checker.clone(),
        caller,
        &document.document_id,
        &state.command.document,
        command,
    )
    .await?;
    synchronize_subject(state).await?;
    Ok(())
}

/// Signs with `method` from now on, for the algorithm of its key, in the document whose DID it names. The method of
/// an added key is only selected while its key can sign. Selecting the selected method again changes nothing.
///
/// # Errors
///
/// Fails when no document has the method's DID, the caller isn't authorized, the method's key isn't available, or
/// the document refuses the method, such as one it doesn't have or whose key names no supported algorithm.
pub async fn select_signing_key(
    state: &IdentityState,
    caller: Caller,
    method: &DidUrl,
) -> Result<(), SelectSigningKeyError> {
    let Some(document) = documents(state).await?.into_values().find(|document| {
        document
            .document
            .as_ref()
            .is_some_and(|did_document| did_document.id == *method.did())
    }) else {
        return Err(SelectSigningKeyError::UnknownDid(method.did().clone()));
    };
    let command = DocumentCommand::SelectSigningKey {
        verification_method_id: method.clone(),
    };
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        &document.document_id,
        &command,
    )
    .await
    .map_err(SelectSigningKeyError::Authorization)?;

    let _guard = state.document_keys_lock.lock().await;
    if document.added_keys.contains_key(method) {
        if let Some(reason) = unavailability(&state.services.subject, method).await {
            return Err(SelectSigningKeyError::NotAvailable {
                method: method.clone(),
                reason,
            });
        }
    }
    command_handler(
        state.authorization_checker.clone(),
        caller,
        &document.document_id,
        &state.command.document,
        command,
    )
    .await?;
    synchronize_subject(state).await?;
    Ok(())
}

/// The key each algorithm of the `did:web` document signs with, in the order of the document's verification methods.
/// Empty without an enabled `did:web` document.
///
/// # Errors
///
/// Fails when the caller may not list the documents, or they can't be loaded.
pub async fn get_signing_keys(state: &IdentityState, caller: Caller) -> Result<Vec<SigningKey>, QueryHandlerError> {
    let documents = query_handler(
        state.authorization_checker.clone(),
        caller,
        "all_documents",
        None,
        &state.query.all_documents,
    )
    .await?
    .unwrap_or_default()
    .documents;
    let Some((document, did_document)) = documents
        .values()
        .filter(|document| is_did_web(document))
        .find_map(|document| Some((document, document.document.as_ref()?)))
    else {
        return Ok(Vec::new());
    };

    let algorithms: Vec<Algorithm> = did_document.methods(None).filter_map(key_algorithm).unique().collect();
    let subject = &state.services.subject;
    let mut keys = Vec::new();
    for algorithm in algorithms {
        let Some(signing) = subject
            .get_verification_method_id(StorageKey::new(SupportedDidMethod::Web, algorithm))
            .await
        else {
            continue;
        };
        let selected = document.signing_methods.get(&algorithm).cloned();
        let problem = match &selected {
            Some(selected) if *selected != signing => obstacle(document, subject, selected)
                .await
                .map(|reason| format!("`{selected}` can't sign: {reason}")),
            _ => match subject.method_key(&signing).await {
                Some(MethodKey::Unavailable(reason) | MethodKey::Gone(reason)) => Some(reason),
                Some(MethodKey::Available(_)) | None => None,
            },
        };
        keys.push(SigningKey {
            algorithm,
            verification_method_id: signing,
            selected_verification_method_id: selected,
            problem,
        });
    }
    Ok(keys)
}

/// Loads the key of every verification method added to the `did:web` document from `keys`, then synchronizes the
/// subject with the documents (see [`synchronize_subject`]). Called once, at startup: a key is only asked for again
/// while it isn't available (see [`retry_unavailable_keys`]), and a key that is loaded isn't checked again until it
/// signs.
///
/// Assumes a single instance: changes another instance makes to the documents' keys aren't picked up, so a second
/// replica would need to reload them from the views.
///
/// # Errors
///
/// Fails when the documents can't be loaded.
pub async fn load_signing_keys(state: &IdentityState, keys: &dyn KeySource) -> Result<(), PersistenceError> {
    let added_keys = did_web_document(state)
        .await?
        .map(|document| document.added_keys)
        .unwrap_or_default();
    let loaded = futures::future::join_all(
        added_keys
            .into_iter()
            .map(|(method, key_id)| async move { (method, load_key(keys, &key_id).await) }),
    )
    .await;

    let _guard = state.document_keys_lock.lock().await;
    set_loaded_keys(&state.services.subject, loaded).await;
    synchronize_subject(state).await
}

/// Asks `keys` again for every key of the `did:web` document that is [`MethodKey::Unavailable`], and returns whether
/// any is still unavailable. A key only counts as available again once it signs. Available and gone keys aren't asked
/// for. Before it asks, the signing cache gives way to the configured keys for every key that failed to sign.
///
/// Keys are asked for without holding `document_keys_lock`, so a slow provider holds up no change to the document; a
/// key added or removed meanwhile keeps that change.
///
/// # Errors
///
/// Fails when the documents can't be loaded.
pub async fn retry_unavailable_keys(state: &IdentityState, keys: &dyn KeySource) -> Result<bool, PersistenceError> {
    {
        let _guard = state.document_keys_lock.lock().await;
        synchronize_subject(state).await?;
    }
    let unavailable = unavailable_keys(state).await?;
    if unavailable.is_empty() {
        return Ok(false);
    }

    let retried = futures::future::join_all(
        unavailable
            .into_iter()
            .map(|(method, key_id)| async move { (method, signing_key(keys, &key_id).await) }),
    )
    .await;

    let _guard = state.document_keys_lock.lock().await;
    set_loaded_keys(&state.services.subject, retried).await;
    synchronize_subject(state).await?;
    Ok(!unavailable_keys(state).await?.is_empty())
}

/// Retries the unavailable keys of the `did:web` document (see [`retry_unavailable_keys`]) right away, then after
/// waits that double from one second up to five minutes, until none is left. It starts again whenever a key fails to
/// sign. Available keys are never asked for. Stops once `state` is dropped.
pub fn spawn_key_retry(state: &Arc<IdentityState>, keys: Arc<dyn KeySource>) -> tokio::task::JoinHandle<()> {
    let key_failures = state.services.subject.key_failures();
    let state = Arc::downgrade(state);
    tokio::spawn(async move {
        loop {
            let mut delay = FIRST_RETRY_DELAY;
            loop {
                let Some(state) = state.upgrade() else { return };
                match retry_unavailable_keys(&state, keys.as_ref()).await {
                    Ok(false) => break,
                    Ok(true) => {}
                    Err(error) => warn!(%error, "Retrying the unavailable signing keys failed"),
                }
                drop(state);
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(MAX_RETRY_DELAY);
            }
            key_failures.notified().await;
        }
    })
}

/// The key `keys` names `key_id`, as the subject keeps it.
async fn load_key(keys: &dyn KeySource, key_id: &str) -> MethodKey {
    match keys.key(key_id).await {
        Ok(signer) => MethodKey::Available(Box::new(signer)),
        Err(error @ KeySourceError::Unavailable { .. }) => MethodKey::Unavailable(error.to_string()),
        Err(error @ (KeySourceError::NotFound(_) | KeySourceError::Unusable { .. })) => {
            MethodKey::Gone(error.to_string())
        }
    }
}

/// The key `keys` names `key_id`, available only once it signs: a key that is there but can't sign, such as one
/// disabled in its provider, stays unavailable.
async fn signing_key(keys: &dyn KeySource, key_id: &str) -> MethodKey {
    let key = load_key(keys, key_id).await;
    let MethodKey::Available(signer) = &key else {
        return key;
    };
    match signer.sign(PROBE).await {
        Ok(_) => key,
        Err(error) => MethodKey::Unavailable(format!("it failed to sign: {error}")),
    }
}

/// Gives the subject the `loaded` keys, except where it holds an available key already, such as one added meanwhile.
async fn set_loaded_keys(subject: &Subject, loaded: Vec<(DidUrl, MethodKey)>) {
    for (method, key) in loaded {
        if matches!(subject.method_key(&method).await, Some(MethodKey::Available(_))) {
            continue;
        }
        match &key {
            MethodKey::Available(_) => {}
            MethodKey::Unavailable(reason) => {
                warn!(%method, reason, "A key of the `did:web` document isn't available; it is retried");
            }
            MethodKey::Gone(reason) => {
                warn!(%method, reason, "A key of the `did:web` document is gone; nothing signs for it")
            }
        }
        subject.set_method_key(method, key).await;
    }
}

/// The added verification methods of the `did:web` document whose key is [`MethodKey::Unavailable`], with their key.
async fn unavailable_keys(state: &IdentityState) -> Result<Vec<(DidUrl, String)>, PersistenceError> {
    let added_keys = did_web_document(state)
        .await?
        .map(|document| document.added_keys)
        .unwrap_or_default();
    let mut unavailable = Vec::new();
    for (method, key_id) in added_keys {
        if matches!(
            state.services.subject.method_key(&method).await,
            Some(MethodKey::Unavailable(_))
        ) {
            unavailable.push((method, key_id));
        }
    }
    Ok(unavailable)
}

/// Makes the subject agree with the documents: it forgets the keys of verification methods that left them, withholds
/// the key of every added method it holds no key for, and points the signing cache of every enabled document at the
/// method that signs for each algorithm (see [`signing_methods`]).
///
/// Call it holding `document_keys_lock`, so that it sees the documents as the subject's keys were changed for.
async fn synchronize_subject(state: &IdentityState) -> Result<(), PersistenceError> {
    let documents = documents(state).await?;
    let subject = &state.services.subject;

    let added: HashSet<&DidUrl> = documents
        .values()
        .flat_map(|document| &document.added_verification_methods)
        .collect();
    subject.retain_method_keys(|method| added.contains(method)).await;
    for method in added {
        if subject.method_key(method).await.is_none() {
            subject
                .set_method_key(method.clone(), MethodKey::Unavailable(NOT_LOADED.to_string()))
                .await;
        }
    }

    for document in documents
        .values()
        .filter(|document| document.status != Status::Disabled)
    {
        let Some(did_method) = document.did_method else {
            continue;
        };
        subject
            .set_signing_methods(did_method, signing_methods(document, subject).await)
            .await;
    }
    Ok(())
}

/// The verification method each algorithm of `document` signs with: the selected one while it can sign (see
/// [`obstacle`]), else the one of the configured issuer key.
async fn signing_methods(document: &Document, subject: &Subject) -> Vec<(Algorithm, DidUrl)> {
    let Some(did_document) = &document.document else {
        return Vec::new();
    };

    let configured_methods: Vec<&VerificationMethod> = did_document
        .methods(None)
        .filter(|method| !document.added_keys.contains_key(&method.id))
        .collect();
    let mut methods = Vec::new();
    for configured in configured_methods {
        let Some(algorithm) = key_algorithm(configured) else {
            continue;
        };
        let mut signing = &configured.id;
        if let Some(selected) = document.signing_methods.get(&algorithm) {
            if obstacle(document, subject, selected).await.is_none() {
                signing = selected;
            }
        }
        methods.push((algorithm, signing.clone()));
    }
    methods
}

/// Why `method` can't sign for `document`, or `None` when it can: it must be in the document, and the key of an added
/// method must be available.
async fn obstacle(document: &Document, subject: &Subject, method: &DidUrl) -> Option<String> {
    let in_document = document
        .document
        .as_ref()
        .is_some_and(|did_document| did_document.verification_method(method).is_some());
    if !in_document {
        return Some("it isn't in the document".to_string());
    }
    if !document.added_keys.contains_key(method) {
        return None;
    }
    unavailability(subject, method).await
}

/// Why the key of the added verification method `method` can't sign, or `None` when it can.
async fn unavailability(subject: &Subject, method: &DidUrl) -> Option<String> {
    match subject.method_key(method).await {
        Some(MethodKey::Available(_)) => None,
        Some(MethodKey::Unavailable(reason) | MethodKey::Gone(reason)) => Some(reason),
        None => Some(NOT_LOADED.to_string()),
    }
}

/// The verification method the key `key_id` was added as, in any document, selected to sign or not: that method signs
/// Domain Linkage Credentials either way. `None` when no document has the key.
///
/// # Errors
///
/// Fails when the documents can't be loaded.
pub async fn verification_method_of_key(
    state: &IdentityState,
    key_id: &str,
) -> Result<Option<DidUrl>, PersistenceError> {
    Ok(documents(state).await?.into_values().find_map(|document| {
        document
            .added_keys
            .into_iter()
            .find(|(_, added)| added == key_id)
            .map(|(method, _)| method)
    }))
}

/// Every document, by id.
async fn documents(state: &IdentityState) -> Result<IndexMap<String, Document>, PersistenceError> {
    Ok(public_query_handler("all_documents", &state.query.all_documents)
        .await?
        .unwrap_or_default()
        .documents)
}

/// The enabled `did:web` document, if there is one.
async fn did_web_document(state: &IdentityState) -> Result<Option<Document>, PersistenceError> {
    Ok(documents(state).await?.into_values().find(is_did_web))
}

fn is_did_web(document: &Document) -> bool {
    document.did_method == Some(SupportedDidMethod::Web)
        && document.status != Status::Disabled
        && document.document.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::aggregate::test_utils::es256_verification_method;
    use crate::document::key_source::NoKeys;
    use crate::services::IdentityServices;
    use crate::state::{identity_state, initialize_documents};
    use agent_shared::config::{config, ToggleOptions};
    use agent_store::in_memory::InMemory;
    use async_trait::async_trait;
    use identity_kit::jose::{JwsAlgorithm, KeyType};
    use identity_kit::keys::{InMemoryKeyStore, KeySigner, KeySpec, KeyStore};
    use shared_kernel::event_bus::EventBusHandle;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tokio::sync::Notify;

    /// Keys in memory, by id. Records every id it is asked for. A key in `unreachable` can't be had now; when asked for
    /// the key `paused` names, it signals `asked` and waits for `resumed`.
    #[derive(Default)]
    struct TestKeys {
        keys: Mutex<HashMap<String, KeySigner<Arc<dyn KeyStore>>>>,
        unreachable: Mutex<HashSet<String>>,
        asked_for: Mutex<Vec<String>>,
        paused: Mutex<Option<String>>,
        asked: Notify,
        resumed: Notify,
    }

    #[async_trait]
    impl KeySource for TestKeys {
        async fn key(&self, id: &str) -> Result<KeySigner<Arc<dyn KeyStore>>, KeySourceError> {
            self.asked_for.lock().unwrap().push(id.to_string());
            if self.paused.lock().unwrap().as_deref() == Some(id) {
                self.asked.notify_one();
                self.resumed.notified().await;
            }
            if self.unreachable.lock().unwrap().contains(id) {
                let reason = "connection refused".to_string();
                return Err(KeySourceError::Unavailable {
                    id: id.to_string(),
                    reason,
                });
            }
            let key = self.keys.lock().unwrap().get(id).cloned();
            key.ok_or_else(|| KeySourceError::NotFound(id.to_string()))
        }
    }

    struct Fixture {
        identity: Arc<IdentityState>,
        keys: Arc<TestKeys>,
    }

    /// An identity without documents.
    async fn identity() -> IdentityState {
        identity_state(&InMemory, IdentityServices::default(), &EventBusHandle::default()).await
    }

    /// An identity with an enabled `did:web` document, and an empty key source.
    async fn fixture() -> Fixture {
        let identity = identity().await;
        let mut configuration = config().clone();
        configuration.did_methods = [(
            SupportedDidMethod::Web,
            ToggleOptions {
                enabled: true,
                preferred: Some(true),
            },
        )]
        .into();
        initialize_documents(&identity, &configuration, &NoKeys).await.unwrap();

        Fixture {
            identity: Arc::new(identity),
            keys: Arc::default(),
        }
    }

    impl Fixture {
        /// A new ES256 key in the key source, by id.
        async fn create_key(&self) -> String {
            let store: Arc<dyn KeyStore> = Arc::new(InMemoryKeyStore::new());
            let key = store
                .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256))
                .await
                .unwrap();
            let id = key.to_string();
            let signer = KeySigner::new(store, key, JwsAlgorithm::ES256).await.unwrap();
            self.keys.keys.lock().unwrap().insert(id.clone(), signer);
            id
        }

        fn signer(&self, key_id: &str) -> KeySigner<Arc<dyn KeyStore>> {
            self.keys.keys.lock().unwrap()[key_id].clone()
        }

        async fn add(&self, key_id: &str) -> DidUrl {
            add_key_to_document(&self.identity, self.keys.as_ref(), Caller::Internal, key_id)
                .await
                .unwrap()
        }

        async fn select(&self, method: &DidUrl) {
            select_signing_key(&self.identity, Caller::Internal, method)
                .await
                .unwrap();
        }

        /// Keeps the key in the key source, but stops it from signing, as disabling it in its provider does.
        async fn disable(&self, key_id: &str) {
            let signer = self.signer(key_id);
            signer.store().delete(signer.key_id()).await.unwrap();
        }

        /// Deletes the key from the key source and its provider.
        async fn delete(&self, key_id: &str) {
            self.disable(key_id).await;
            self.keys.keys.lock().unwrap().remove(key_id);
        }

        /// Signs for `method`, as issuance does.
        async fn sign(&self, method: &DidUrl) {
            let _ = self
                .identity
                .services
                .subject
                .sign_for_method(method, Algorithm::ES256, b"message")
                .await;
        }

        /// Forgets what the subject knows, as a restart does, and loads the keys again as startup does.
        async fn restart(&self) {
            let subject = &self.identity.services.subject;
            subject.retain_method_keys(|_| false).await;
            subject.verification_method_ids.lock().await.clear();
            load_signing_keys(&self.identity, self.keys.as_ref()).await.unwrap();
        }

        async fn retry(&self) -> bool {
            retry_unavailable_keys(&self.identity, self.keys.as_ref())
                .await
                .unwrap()
        }

        /// The verification method that signs for ES256 in the `did:web` document now.
        async fn es256_signing_method(&self) -> DidUrl {
            self.identity
                .services
                .subject
                .get_verification_method_id(StorageKey::new(SupportedDidMethod::Web, Algorithm::ES256))
                .await
                .unwrap()
        }

        async fn method_key(&self, method: &DidUrl) -> Option<MethodKey> {
            self.identity.services.subject.method_key(method).await
        }
    }

    fn configured_es256_method() -> DidUrl {
        es256_verification_method::default().id
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn adding_a_key_names_its_verification_method_by_its_thumbprint() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;

        let method = fixture.add(&key).await;

        let thumbprint = fixture.signer(&key).public_jwk().thumbprint_sha256().unwrap();
        assert_eq!(method.fragment(), Some(thumbprint.as_str()));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn adding_a_key_without_a_did_web_document_fails() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;

        let result = add_key_to_document(&identity().await, fixture.keys.as_ref(), Caller::Internal, &key).await;

        assert!(matches!(result, Err(AddKeyError::NoDidWebDocument)));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn adding_an_unknown_key_fails() {
        let fixture = fixture().await;

        let result = add_key_to_document(&fixture.identity, fixture.keys.as_ref(), Caller::Internal, "missing").await;

        assert!(matches!(result, Err(AddKeyError::Key(KeySourceError::NotFound(_)))));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn an_added_key_can_be_selected_right_away() {
        let fixture = fixture().await;
        let method = fixture.add(&fixture.create_key().await).await;

        let result = select_signing_key(&fixture.identity, Caller::Internal, &method).await;

        assert!(result.is_ok(), "{result:?}");
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn an_added_key_does_not_sign_until_it_is_selected() {
        let fixture = fixture().await;

        fixture.add(&fixture.create_key().await).await;

        assert_eq!(fixture.es256_signing_method().await, configured_es256_method());
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn selecting_a_key_signs_with_it_right_away() {
        let fixture = fixture().await;
        let method = fixture.add(&fixture.create_key().await).await;

        fixture.select(&method).await;

        assert_eq!(fixture.es256_signing_method().await, method);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn deselecting_a_key_signs_with_the_configured_key_right_away() {
        let fixture = fixture().await;
        let method = fixture.add(&fixture.create_key().await).await;
        fixture.select(&method).await;

        deselect_signing_key(&fixture.identity, Caller::Internal, Algorithm::ES256)
            .await
            .unwrap();

        assert_eq!(fixture.es256_signing_method().await, configured_es256_method());
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn selecting_a_key_of_an_unknown_did_fails() {
        let fixture = fixture().await;

        let result = select_signing_key(
            &fixture.identity,
            Caller::Internal,
            &"did:web:other.example#key".parse().unwrap(),
        )
        .await;

        assert!(
            matches!(result, Err(SelectSigningKeyError::UnknownDid(did)) if did.as_str() == "did:web:other.example")
        );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn selecting_a_key_that_is_not_available_fails() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;
        fixture.disable(&key).await;
        fixture.sign(&method).await;

        let result = select_signing_key(&fixture.identity, Caller::Internal, &method).await;

        assert!(matches!(result, Err(SelectSigningKeyError::NotAvailable { .. })));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_selected_key_signs() {
        let fixture = fixture().await;
        let method = fixture.add(&fixture.create_key().await).await;
        fixture.select(&method).await;

        fixture.restart().await;

        assert_eq!(fixture.es256_signing_method().await, method);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_selected_key_that_is_not_available_does_not_sign() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;
        fixture.select(&method).await;
        fixture.delete(&key).await;

        fixture.restart().await;

        assert_eq!(fixture.es256_signing_method().await, configured_es256_method());
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_selected_key_that_left_the_document_does_not_sign() {
        let fixture = fixture().await;
        let mut document = did_web_document(&fixture.identity).await.unwrap().unwrap();
        document.signing_methods.insert(
            Algorithm::ES256,
            "did:web:my-domain.example.org#replaced-key".parse().unwrap(),
        );

        let methods = signing_methods(&document, &fixture.identity.services.subject).await;

        assert!(methods.contains(&(Algorithm::ES256, configured_es256_method())));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn loads_the_key_of_an_added_verification_method() {
        let fixture = fixture().await;
        let method = fixture.add(&fixture.create_key().await).await;

        fixture.restart().await;

        assert!(matches!(
            fixture.method_key(&method).await,
            Some(MethodKey::Available(_))
        ));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn does_not_load_a_key_its_store_no_longer_holds() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;
        fixture.delete(&key).await;

        fixture.restart().await;

        assert!(matches!(fixture.method_key(&method).await, Some(MethodKey::Gone(_))));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn keys_that_fail_to_load_leave_the_added_verification_methods_without_a_key() {
        let fixture = fixture().await;
        let method = fixture.add(&fixture.create_key().await).await;
        fixture.identity.services.subject.retain_method_keys(|_| false).await;

        load_signing_keys(&fixture.identity, &NoKeys).await.unwrap();

        assert!(matches!(fixture.method_key(&method).await, Some(MethodKey::Gone(_))));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_key_whose_provider_cannot_be_reached_at_startup_is_left_to_retry() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;
        fixture.keys.unreachable.lock().unwrap().insert(key);

        fixture.restart().await;

        assert!(matches!(
            fixture.method_key(&method).await,
            Some(MethodKey::Unavailable(_))
        ));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_selected_key_that_fails_to_sign_stops_signing() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;
        fixture.select(&method).await;
        fixture.disable(&key).await;
        fixture.sign(&method).await;

        fixture.retry().await;

        assert_eq!(fixture.es256_signing_method().await, configured_es256_method());
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn an_unavailable_key_is_retried_and_comes_back() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;
        fixture.keys.unreachable.lock().unwrap().insert(key.clone());
        fixture.restart().await;
        fixture.keys.unreachable.lock().unwrap().remove(&key);

        fixture.retry().await;

        assert!(matches!(
            fixture.method_key(&method).await,
            Some(MethodKey::Available(_))
        ));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_retried_key_that_is_there_but_cannot_sign_stays_unavailable() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;
        fixture.disable(&key).await;
        fixture.sign(&method).await;

        fixture.retry().await;

        assert!(matches!(
            fixture.method_key(&method).await,
            Some(MethodKey::Unavailable(_))
        ));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_key_that_stays_unavailable_is_left_to_retry() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        fixture.add(&key).await;
        fixture.keys.unreachable.lock().unwrap().insert(key);
        fixture.restart().await;

        let left = fixture.retry().await;

        assert!(left);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_key_gone_from_its_source_is_not_left_to_retry() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;
        fixture.delete(&key).await;
        fixture.sign(&method).await;

        let left = fixture.retry().await;

        assert!(!left);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn retrying_asks_for_no_available_key() {
        let fixture = fixture().await;
        let healthy = fixture.create_key().await;
        fixture.add(&healthy).await;
        let failing = fixture.create_key().await;
        let failing_method = fixture.add(&failing).await;
        fixture.disable(&failing).await;
        fixture.sign(&failing_method).await;
        fixture.keys.asked_for.lock().unwrap().clear();

        fixture.retry().await;

        assert_eq!(*fixture.keys.asked_for.lock().unwrap(), [failing]);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn a_retry_started_before_a_key_is_added_keeps_it() {
        let fixture = fixture().await;
        let first = fixture.create_key().await;
        fixture.add(&first).await;
        fixture.keys.unreachable.lock().unwrap().insert(first.clone());
        fixture.restart().await;
        *fixture.keys.paused.lock().unwrap() = Some(first);
        let retry = tokio::spawn({
            let identity = fixture.identity.clone();
            let keys = fixture.keys.clone();
            async move { retry_unavailable_keys(&identity, keys.as_ref()).await }
        });
        fixture.keys.asked.notified().await;
        let second = fixture.add(&fixture.create_key().await).await;
        fixture.keys.resumed.notify_one();

        retry.await.unwrap().unwrap();

        assert!(matches!(
            fixture.method_key(&second).await,
            Some(MethodKey::Available(_))
        ));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn finds_the_verification_method_of_a_key_that_is_not_selected() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;

        let found = verification_method_of_key(&fixture.identity, &key).await.unwrap();

        assert_eq!(found, Some(method));
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn reports_a_selected_key_that_is_not_available() {
        let fixture = fixture().await;
        let key = fixture.create_key().await;
        let method = fixture.add(&key).await;
        fixture.select(&method).await;
        fixture.disable(&key).await;
        fixture.sign(&method).await;
        fixture.retry().await;

        let signing_keys = get_signing_keys(&fixture.identity, Caller::Internal).await.unwrap();

        let es256 = signing_keys
            .into_iter()
            .find(|signing_key| signing_key.algorithm == Algorithm::ES256)
            .unwrap();
        assert_eq!(
            (
                es256.verification_method_id,
                es256.selected_verification_method_id,
                es256.problem.is_some()
            ),
            (configured_es256_method(), Some(method), true)
        );
    }
}

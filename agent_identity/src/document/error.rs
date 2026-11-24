use agent_shared::config::SupportedDidMethod;
use identity_kit::did::DidUrl;
use identity_kit::keys::KeyId;
use jsonwebtoken::Algorithm;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DocumentError {
    // TODO: Add more specific errors as needed
    #[error("Error while handling document command: {0}")]
    GenericError(String),
    #[error("Error while producing DID document: {0}")]
    ProduceDocumentError(#[from] identity_kit::did::DocumentError),
    #[error("Missing document")]
    MissingDocumentError,
    #[error("Missing did_method")]
    MissingDidMethodError,
    #[error("Error while adding service: {0}")]
    AddServiceError(String),
    #[error("Invalid DID: {0}")]
    InvalidDidError(String),
    #[error("Error while building Verification Method: {0}")]
    VerificationMethodBuilderError(String),
    #[error("Unsupported signing algorithm: {0:?}")]
    UnsupportedSigningAlgorithmError(Algorithm),
    #[error("A fixed algorithm is required for this DID method: `{0}`")]
    MissingFixedAlgorithmError(SupportedDidMethod),
    #[error("Key not found: {0}")]
    MissingKeyError(String),
    #[error("Error while inserting Verification Method: {0}")]
    VerificationMethodInsertionError(String),
    #[error("Failed to generate DID from key with Key ID: {0}")]
    GenerateDidError(KeyId),

    // did:web specific Errors
    #[error("Opaque origin not supported")]
    OpaqueOriginError,
    #[error("Host must be a domain name")]
    HostError,
    #[error("Invalid domain or origin: {0}")]
    InvalidOriginError(String),

    // Errors of added keys and signing keys
    /// Keys are only added to `did:web` documents, the only ones that change.
    #[error("Only `did:web` documents add public keys")]
    DidWebOnlyError,
    /// The key of the verification method names no algorithm UniCore signs with.
    #[error("The key of `{0}` names no supported algorithm")]
    NoKeyAlgorithmError(DidUrl),
    /// Only a verification method of the document can be selected to sign.
    #[error("The document has no verification method `{0}`")]
    UnknownVerificationMethodError(DidUrl),
    /// The verification method is one of the configured issuer keys, or isn't in the document.
    #[error("`{0}` is not the verification method of an added key")]
    NotAnAddedKeyError(DidUrl),
    /// The verification method is selected to sign for its algorithm, which would be left without its key.
    #[error("`{0}` is the selected signing key of its algorithm; select another key or deselect it first")]
    SelectedSigningKeyError(DidUrl),
}

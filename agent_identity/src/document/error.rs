use agent_shared::config::SupportedDidMethod;
use identity_iota::storage::KeyId;
use jsonwebtoken::Algorithm;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DocumentError {
    #[error("Error while producing DID document: {0}")]
    ProduceDocumentError(#[from] identity_document::error::Error),
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

    // did:iota: specific Errors
    #[error("Invalid Node Endpoint: {0}")]
    InvalidNodeEndpointError(String),
    #[error("Iota Client Builder error: {0}")]
    IotaClientBuilderError(String),
    #[error("Missing required network name for method: {0}")]
    MissingNetworkNameError(SupportedDidMethod),
    #[error("Error while retrieving the Wallet Address: {0}")]
    WalletAddressError(String),
    #[error("Iota Client error: {0}")]
    IotaClientError(#[from] identity_iota::iota::Error),
    #[error("Error while building the Alias Output: {0}")]
    AliasOutputBuilderError(String),
    #[error(
        "Failed to publish DID Document due to insufficient deposit.\n\
        Please ensure the associated {0} address is adequately funded.\n\
        {0} address: `{1}`"
    )]
    InsufficientDepositError(String, String),

    // did:web specific Errors
    #[error("Opaque origin not supported")]
    OpaqueOriginError,
    #[error("Host must be a domain name")]
    HostError,
}

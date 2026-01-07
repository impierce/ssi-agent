use agent_shared::config::SupportedDidMethod;
use identity_iota::storage::KeyId;
use jsonwebtoken::Algorithm;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DocumentError {
    // TODO: Add more specific errors as needed
    #[error("Error while handling document command: {0}")]
    GenericError(String),
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
    #[error("Iota Controler error: {0}")]
    IotaControllerError(identity_iota::iota::rebased::Error),
    #[error("Iota Identity error: {0}")]
    IotaIdentityError(#[from] identity_iota::iota::rebased::Error),
    #[error("Iota Product Common error: {0}")]
    IotaProductCommonError(#[from] product_common::error::Error),
    #[error("Failed to publish IOTA DID Document: {0}")]
    IotaPublishDocumentError(String),
    #[error("Failed to update IOTA DID Document: {0}")]
    IotaUpdateDocumentError(String),
    #[error("Falied to deactivate IOTA DID: {0}")]
    IotaDeactivateDidError(String),

    // did:web specific Errors
    #[error("Opaque origin not supported")]
    OpaqueOriginError,
    #[error("Host must be a domain name")]
    HostError,
}

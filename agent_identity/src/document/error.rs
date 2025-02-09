use jsonwebtoken::Algorithm;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DocumentError {
    #[error("Error while producing DID document: {0}")]
    ProduceDocumentError(String),
    #[error("Missing document")]
    MissingDocumentError,
    #[error("Error while adding service: {0}")]
    AddServiceError(String),
    #[error("Invalid DID: {0}")]
    InvalidDidError(String),
    #[error("Did Method is not updateable: {0}")]
    MethodNotUpdateableError(String),
    #[error("Error while building Verification Method: {0}")]
    VerificationMethodBuilderError(String),
    #[error("Unsupported signing algorithm: {0:?}")]
    UnsupportedSigningAlgorithmError(Algorithm),
    #[error("Public Key Jwk is missing the required `kid` parameter")]
    MissingKidError,
    #[error("Public Key Jwk is missing the required `alg` parameter")]
    MissingAlgError,
    #[error("Error while inserting Verification Method: {0}")]
    VerificationMethodInsertionError(String),

    // did:iota: specific Errors
    #[error("Invalid Node Endpoint: {0}")]
    InvalidNodeEndpointError(String),
    #[error("Iota Client Builder error: {0}")]
    IotaClientBuilderError(String),
    #[error("Error while initializing the Secret Manager: {0}")]
    SecretManagerInitializationError(String),
    #[error("Iota Client error: {0}")]
    IotaClientError(#[from] identity_iota::iota::Error),
    #[error("Error while building the Alias Output")]
    AliasOutputBuilderError,
    #[error("Error while building the Secret Manager")]
    SecretManagerBuilderError,

    // did:web specific Errors
    #[error("Opaque origin not supported")]
    OpaqueOriginError,
    #[error("Host must be a domain name")]
    HostError,
}

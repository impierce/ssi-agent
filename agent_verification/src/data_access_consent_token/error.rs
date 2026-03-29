use thiserror::Error;

// As the handler will show, there are no operations taking place that can fail within the handler itself so therefore this error enum is currently empty.
#[derive(Error, Debug)]
pub enum DataAccessConsentTokenError {
    #[error("Data Access Consent Token with id {0} not found")]
    DataAccessConsentTokenNotFound(String),
    #[error("Error resolving DID: {0}")]
    DidResolutionError(String),
    #[error("Invalid Data Access Endpoint response: {0}")]
    InvalidResponse(String),
    #[error("Data Access Consent Token error: {0}")]
    DACTError(String),
    #[error("No Data Access Endpoint found: {0}")]
    NoDataAccessEndpointFound(String),
    #[error("Query error: {0}")]
    QueryError(String),
    // TODO: This error probably is obsolete since validation errors are now handled by the `public_verification_response`.
    #[error("Validation error: {0}")]
    ValidationError(String),
}

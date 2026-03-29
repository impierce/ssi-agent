use thiserror::Error;

// As the handler will show, there are no operations taking place that can fail within the handler itself so therefore this error enum is currently empty.
#[derive(Error, Debug)]
pub enum DataAccessConsentTokenError {
    #[error("Data Access Consent Token with id {0} not found")]
    DataAccessConsentTokenNotFound(String),
    #[error("Error resolving DID: {0}")]
    DidResolutionError(String),
    #[error("JWT decoding error: {0}")]
    JwtDecodingError(String),
    #[error("No Data Access Endpoint found: {0}")]
    NoDataAccessEndpointFound(String),
    #[error("Query error: {0}")]
    QueryError(String),
}

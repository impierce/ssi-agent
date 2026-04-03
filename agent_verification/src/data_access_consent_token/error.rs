use thiserror::Error;

#[derive(Error, Debug)]
pub enum DataAccessConsentTokenError {
    #[error("A Data Access Consent Token with the given ID already exists: {0}")]
    DACTAlreadyExists(String),
    #[error("Data Access Consent Token error: {0}")]
    DACTError(String),
    #[error("Data Access Consent Token with id {0} not found")]
    DataAccessConsentTokenNotFound(String),
    #[error("Error fetching a response from the Data Access Endpoint: {0}")]
    DataAccessEndpointFetchError(String),
    #[error("Error resolving DID: {0}")]
    DidResolutionError(String),
    #[error("Data Access  Endpoint is not enabled in the configuration")]
    EndpointNotEnabled,
    #[error("Invalid Data Access Endpoint response: {0}")]
    InvalidResponse(String),
    #[error("No Data Access Endpoint found: {0}")]
    NoDataAccessEndpointFound(String),
    #[error("Query error: {0}")]
    QueryError(String),
}

use oauth_tsl::error::OAuthTSLError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StatusListError {
    #[error("Failed to set index `{0}` due to error: {1}")]
    FailedToSetIndex(usize, String),
    #[error("Failed to Gzip compress the JWT token.")]
    GzipCompressionError,
    #[error("Failed to encode the status list token as JWT.")]
    JwtEncodeError,
    #[error("Failed to encode and compress the status list claim: {0:?}")]
    StatusListEncodingError(OAuthTSLError),
    #[error("Status list not found for the provided id: {0}")]
    StatusListNotFound(String),
    #[error("Error querying the status list")]
    StatusListQueryError,
    #[error("Failed to parse the `sub` url for the status list")]
    StatusListUrlParsingError,
}

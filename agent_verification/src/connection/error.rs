use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("Invalid SIOPv2 authorization response: {0}")]
    InvalidSIOPv2AuthorizationResponse(#[source] anyhow::Error),
}

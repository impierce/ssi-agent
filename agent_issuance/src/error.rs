use thiserror::Error;

#[derive(Error, Debug)]
pub enum IssuanceError {
    #[error("Missing Credential Issuer Metadata")]
    MissingCredentialIssuerMetadataError,
    #[error("Missing OAuth Authorization Server Metadata")]
    MissingAuthorizationServerMetadataError,
    #[error("Invalid Pre-Authorized Code")]
    InvalidPreAuthorizedCodeError,
    #[error("Invalid Access Token")]
    InvalidAccessTokenError,
    #[error("Credential must be an object")]
    InvalidCredentialError,
    #[error("Missing `Proof` in Credential Request")]
    MissingProofError,
    #[error("Invalid `Proof` in Credential Request")]
    InvalidProofError,
    #[error("Missing `iss` claim in `Proof`")]
    MissingProofIssuerError,
    #[error("Cannot find Issuance Subject with `subject_id`: {0}")]
    MissingIssuanceSubjectError(uuid::Uuid),
}

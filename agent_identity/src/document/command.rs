use super::aggregate::Status;
use agent_shared::config::SupportedDidMethod;
use identity_kit::did::{Did, DidUrl, Service as DocumentService};
use identity_kit::jose::Jwk;
use jsonwebtoken::Algorithm;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DocumentCommand {
    CreateDocument {
        document_id: String,
        did_method: SupportedDidMethod,
        with_fixed_algorithm: Option<Algorithm>,
    },
    OverwritePreviousDidWeb {
        previous_did: Did,
        public_url: url::Url,
    },
    UpdateDocumentStatus {
        status: Status,
    },
    UpdatePublicKeys {
        public_key_jwks: Vec<Jwk>,
    },
    /// Adds a verification method for a key that isn't a configured issuer key, such as a KMS key. Only `did:web`
    /// documents can add keys, and the keys stay when the configured ones are updated.
    AddPublicKey {
        public_key_jwk: Box<Jwk>,
    },
    /// Signs with the verification method `verification_method_id` from now on, for the algorithm of its key.
    SelectSigningKey {
        verification_method_id: DidUrl,
    },
    AddService {
        service_id: String,
        service: Box<DocumentService>,
    },
    RemoveService {
        service_id: String,
    },
    PublishDocument,
}

impl CommandOperation for DocumentCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateDocument { .. } => "identity.documents.create",
            Self::OverwritePreviousDidWeb { .. } => "identity.documents.previous_did_web.overwrite",
            Self::UpdateDocumentStatus { .. } => "identity.documents.status.update",
            Self::UpdatePublicKeys { .. } => "identity.documents.public_keys.update",
            Self::AddPublicKey { .. } => "identity.documents.public_keys.add",
            Self::SelectSigningKey { .. } => "identity.documents.signing_key.select",
            Self::AddService { .. } => "identity.documents.services.add",
            Self::RemoveService { .. } => "identity.documents.services.remove",
            Self::PublishDocument => "identity.documents.publish",
        }
    }
}

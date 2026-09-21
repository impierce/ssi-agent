use super::aggregate::Status;
use agent_shared::config::SupportedDidMethod;
use identity_did::CoreDID;
use identity_document::service::Service as DocumentService;
use identity_iota::verification::jwk::Jwk;
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
        previous_did: CoreDID,
        public_url: url::Url,
    },
    UpdateDocumentStatus {
        status: Status,
    },
    UpdatePublicKeys {
        public_key_jwks: Vec<Jwk>,
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
            Self::AddService { .. } => "identity.documents.services.add",
            Self::RemoveService { .. } => "identity.documents.services.remove",
            Self::PublishDocument => "identity.documents.publish",
        }
    }
}

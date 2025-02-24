use super::aggregate::Status;
use agent_shared::config::SupportedDidMethod;
use identity_document::service::Service as DocumentService;
use identity_iota::verification::jwk::Jwk;
use jsonwebtoken::Algorithm;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DocumentCommand {
    CreateDocument {
        document_id: String,
        did_method: SupportedDidMethod,
        with_fixed_algorithm: Option<Algorithm>,
    },
    UpdateDocumentStatus {
        document_id: String,
        status: Status,
    },
    UpdatePublicKeys {
        document_id: String,
        public_key_jwks: Vec<Jwk>,
    },
    AddService {
        service_id: String,
        service: DocumentService,
    },
    RemoveService {
        service_id: String,
    },
    PublishDocument {
        document_id: String,
    },
}

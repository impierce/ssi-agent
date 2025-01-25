use super::aggregate::Status;
use identity_document::service::Service as DocumentService;
use identity_iota::verification::jwk::Jwk;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DocumentCommand {
    CreateDocument {
        document_id: String,
    },
    SetPublicKeyJwks {
        document_id: String,
        public_key_jwks: Vec<Jwk>,
    },
    SetStatus {
        document_id: String,
        status: Status,
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

use super::aggregate::Status;
use agent_shared::config::SupportedDidMethod;
use identity_document::service::Service as DocumentService;
use identity_iota::verification::jwk::Jwk;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DocumentCommand {
    CreateDocument {
        did_method: SupportedDidMethod,
    },
    SetPublicKeyJwks {
        did_method: SupportedDidMethod,
        public_key_jwks: Vec<Jwk>,
    },
    SetStatus {
        did_method: SupportedDidMethod,
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
        did_method: SupportedDidMethod,
    },
}

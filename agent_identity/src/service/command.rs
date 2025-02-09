use identity_iota::document::CoreDocument;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServiceCommand {
    CreateDomainLinkageService {
        service_id: String,
        documents: Vec<CoreDocument>,
    },
    DeleteDomainLinkageService {
        service_id: String,
    },
    CreateLinkedVerifiablePresentationService {
        service_id: String,
        presentation_ids: Vec<String>,
    },
}

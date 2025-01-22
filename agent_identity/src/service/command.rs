use crate::document::aggregate::Document;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServiceCommand {
    CreateDomainLinkageService {
        service_id: String,
        documents: Vec<Document>,
    },
    DeleteDomainLinkageService {
        service_id: String,
    },
    CreateLinkedVerifiablePresentationService {
        service_id: String,
        presentation_ids: Vec<String>,
    },
}

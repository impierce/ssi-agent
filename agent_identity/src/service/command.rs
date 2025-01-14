use serde::Deserialize;

use crate::document::aggregate::Document;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServiceCommand {
    CreateDomainLinkageService {
        service_id: String,
        documents: Vec<Document>,
    },
    CreateLinkedVerifiablePresentationService {
        service_id: String,
        presentation_ids: Vec<String>,
    },
}

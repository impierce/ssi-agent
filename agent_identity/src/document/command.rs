use identity_document::service::ServiceEndpoint;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DocumentCommand {
    CreateDocument {
        document_id: String,
    },
    AddService {
        service_id: String,
        type_: String,
        service_endpoint: ServiceEndpoint,
    },
    PublishDocument {
        document_id: String,
    },
}

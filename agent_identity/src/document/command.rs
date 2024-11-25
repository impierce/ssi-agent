use did_manager::DidMethod;
use identity_document::service::Service as DocumentService;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DocumentCommand {
    CreateDocument { did_method: DidMethod },
    AddService { service: DocumentService },
}

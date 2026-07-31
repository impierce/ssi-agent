use identity_iota::verification::VerificationMethod;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServiceCommand {
    CreateDomainLinkageService {
        service_id: String,
        verification_methods: Vec<VerificationMethod>,
    },
    DeleteDomainLinkageService {
        service_id: String,
    },
    CreateLinkedVerifiablePresentationService {
        service_id: String,
        presentation_ids: Vec<String>,
    },
}

impl CommandOperation for ServiceCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateDomainLinkageService { .. } => "identity.services.domain_linkage.create",
            Self::DeleteDomainLinkageService { .. } => "identity.services.domain_linkage.delete",
            Self::CreateLinkedVerifiablePresentationService { .. } => {
                "identity.services.linked_verifiable_presentation.create"
            }
        }
    }
}

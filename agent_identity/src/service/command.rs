use identity_iota::verification::VerificationMethod;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServiceCommand {
    CreateDomainLinkageService {
        service_id: String,
        verification_methods: Vec<VerificationMethod>,
    },
    ReissueDomainLinkageService {
        service_id: String,
        verification_methods: Vec<VerificationMethod>,
        only_if_expiring: bool,
    },
    DeleteDomainLinkageService {
        service_id: String,
    },
    CreateLinkedVerifiablePresentationService {
        service_id: String,
        presentation_ids: Vec<String>,
    },
    DeleteLinkedVerifiablePresentationService {
        service_id: String,
    },
}

impl ServiceCommand {
    pub fn operation(&self) -> &'static str {
        match self {
            Self::CreateDomainLinkageService { .. } => "identity.services.domain_linkage.create",
            Self::ReissueDomainLinkageService { .. } => "identity.services.domain_linkage.reissue",
            Self::DeleteDomainLinkageService { .. } => "identity.services.domain_linkage.delete",
            Self::CreateLinkedVerifiablePresentationService { .. } => {
                "identity.services.linked_verifiable_presentation.create"
            }
            Self::DeleteLinkedVerifiablePresentationService { .. } => {
                "identity.services.linked_verifiable_presentation.delete"
            }
        }
    }

    pub fn service_id(&self) -> &str {
        match self {
            Self::CreateDomainLinkageService { service_id, .. }
            | Self::ReissueDomainLinkageService { service_id, .. }
            | Self::DeleteDomainLinkageService { service_id }
            | Self::CreateLinkedVerifiablePresentationService { service_id, .. }
            | Self::DeleteLinkedVerifiablePresentationService { service_id } => service_id,
        }
    }
}

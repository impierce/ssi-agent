use identity_iota::verification::VerificationMethod;
use serde::Deserialize;

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
    CreateDataAccessEndpointService {
        service_id: String,
    },
    DeleteDataAccessEndpointService {
        service_id: String,
    },
    CreatePublicVerificationEndpointService {
        service_id: String,
    },
    DeletePublicVerificationEndpointService {
        service_id: String,
    },
}

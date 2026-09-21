use identity_iota::verification::VerificationMethod;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServiceCommand {
    /// Links the given origins, in addition to any already linked. Adding an origin that is already
    /// linked is a no-op; there is no cap and no conflict.
    AddLinkedDomains {
        service_id: String,
        verification_methods: Vec<VerificationMethod>,
        origins: Vec<Url>,
    },
    /// Re-issues the credentials for the currently linked origins. Never changes which origins are
    /// linked.
    RenewLinkedDomainsCredentials {
        service_id: String,
        verification_methods: Vec<VerificationMethod>,
        only_if_expiring: bool,
    },
    /// Unlinks the given origins. Removing an origin that is not linked is a no-op. Removing the
    /// last remaining origin deletes the service.
    RemoveLinkedDomains {
        service_id: String,
        origins: Vec<Url>,
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
            Self::AddLinkedDomains { .. } => "identity.services.linked_domains.add",
            Self::RenewLinkedDomainsCredentials { .. } => "identity.services.linked_domains.renew",
            Self::RemoveLinkedDomains { .. } => "identity.services.linked_domains.remove",
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
            Self::AddLinkedDomains { service_id, .. }
            | Self::RenewLinkedDomainsCredentials { service_id, .. }
            | Self::RemoveLinkedDomains { service_id, .. }
            | Self::CreateLinkedVerifiablePresentationService { service_id, .. }
            | Self::DeleteLinkedVerifiablePresentationService { service_id } => service_id,
        }
    }
}

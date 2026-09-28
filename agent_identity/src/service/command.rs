use crate::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID;
use identity_iota::verification::VerificationMethod;
use shared_kernel::authorization::CommandOperation;
use url::Url;

#[derive(Debug)]
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
    RemoveLinkedDomains { service_id: String, origins: Vec<Url> },
    /// Publishes the given presentations in addition to any already published. Adding an already
    /// published presentation is a no-op and creates the service when necessary.
    AddLinkedVerifiablePresentations { presentation_ids: Vec<String> },
    /// Stops publishing the given presentations. Removing an unpublished presentation is a no-op;
    /// removing the last published presentation deletes the service.
    RemoveLinkedVerifiablePresentations { presentation_ids: Vec<String> },
}

impl ServiceCommand {
    pub fn operation(&self) -> &'static str {
        match self {
            Self::AddLinkedDomains { .. } => "identity.services.linked_domains.add",
            Self::RenewLinkedDomainsCredentials { .. } => "identity.services.linked_domains.renew",
            Self::RemoveLinkedDomains { .. } => "identity.services.linked_domains.remove",
            Self::AddLinkedVerifiablePresentations { .. } => "identity.services.linked_verifiable_presentation.add",
            Self::RemoveLinkedVerifiablePresentations { .. } => {
                "identity.services.linked_verifiable_presentation.remove"
            }
        }
    }

    pub fn service_id(&self) -> &str {
        match self {
            Self::AddLinkedDomains { service_id, .. }
            | Self::RenewLinkedDomainsCredentials { service_id, .. }
            | Self::RemoveLinkedDomains { service_id, .. } => service_id,
            Self::AddLinkedVerifiablePresentations { .. } | Self::RemoveLinkedVerifiablePresentations { .. } => {
                LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID
            }
        }
    }
}

impl CommandOperation for ServiceCommand {
    fn operation_name(&self) -> &'static str {
        self.operation()
    }
}

use super::aggregate::Status;
use agent_shared::config::SupportedDidMethod;
use identity_kit::did::{Did, DidUrl, Service as DocumentService};
use identity_kit::jose::Jwk;
use jsonwebtoken::Algorithm;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;
use strum::EnumDiscriminants;

#[derive(Debug, Deserialize, EnumDiscriminants)]
#[serde(untagged)]
#[strum_discriminants(name(DocumentOperation))]
pub enum DocumentCommand {
    CreateDocument {
        document_id: String,
        did_method: SupportedDidMethod,
        with_fixed_algorithm: Option<Algorithm>,
    },
    OverwritePreviousDidWeb {
        previous_did: Did,
        public_url: url::Url,
    },
    UpdateDocumentStatus {
        status: Status,
    },
    UpdatePublicKeys {
        public_key_jwks: Vec<Jwk>,
    },
    AddService {
        service_id: String,
        service: Box<DocumentService>,
    },
    RemoveService {
        service_id: String,
    },
    /// Adds a verification method for `public_key`, a key that isn't a configured issuer key, such as the key the Key
    /// Management System names `key_id`. The method is named by the key's thumbprint and doesn't sign until it is
    /// selected. `did:web` only; adding a key again adds nothing.
    AddPublicKey {
        public_key: Box<Jwk>,
        key_id: String,
    },
    /// Removes the verification method of an added key. The selected signing key of an algorithm isn't removed.
    RemovePublicKey {
        verification_method_id: DidUrl,
    },
    /// Signs with the verification method from now on, for the algorithm of its key. Selecting the selected method
    /// again selects nothing.
    SelectSigningKey {
        verification_method_id: DidUrl,
    },
    /// Signs with the configured issuer key of `algorithm` again. Without a selected method, nothing changes.
    DeselectSigningKey {
        algorithm: Algorithm,
    },
}

/// A document command's operation is known before the command is: adding a key is authorized before the Key
/// Management System is asked for its public key.
impl CommandOperation for DocumentOperation {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateDocument => "identity.documents.create",
            Self::OverwritePreviousDidWeb => "identity.documents.previous_did_web.overwrite",
            Self::UpdateDocumentStatus => "identity.documents.status.update",
            Self::UpdatePublicKeys => "identity.documents.public_keys.update",
            Self::AddService => "identity.documents.services.add",
            Self::RemoveService => "identity.documents.services.remove",
            Self::AddPublicKey => "identity.documents.public_keys.add",
            Self::RemovePublicKey => "identity.documents.public_keys.remove",
            Self::SelectSigningKey => "identity.documents.signing_keys.select",
            Self::DeselectSigningKey => "identity.documents.signing_keys.deselect",
        }
    }
}

impl CommandOperation for DocumentCommand {
    fn operation_name(&self) -> &'static str {
        DocumentOperation::from(self).operation_name()
    }
}

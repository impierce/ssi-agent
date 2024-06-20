use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use serde::Deserialize;

use super::entity::Data;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CredentialCommand {
    CreateUnsignedCredential {
        data: Data,
        credential_configuration: CredentialConfigurationsSupportedObject,
    },
    CreateSignedCredential {
        signed_credential: serde_json::Value,
    },
    SignCredential {
        subject_id: String,
        // When true, a credential will be re-signed if it already exists.
        overwrite: bool,
    },
}

//! What a key provider is set up with when it is added. Settings say where the provider keeps keys and who this
//! instance acts as there; none of them are secret, so they are recorded in events and shown through the API.

use crate::key_provider::aggregate::KeyProviderType;
use serde::{Deserialize, Deserializer, Serialize};
use strum::Display;
use utoipa::ToSchema;

/// The settings of a key provider. None of them are secret. Only `stackit_kms` providers have any.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, ToSchema)]
#[serde(untagged)]
pub enum KeyProviderSettings {
    StackitKms(StackitKmsSettings),
}

impl KeyProviderSettings {
    /// The provider type these settings are for.
    #[must_use]
    pub fn provider_type(&self) -> KeyProviderType {
        match self {
            Self::StackitKms(_) => KeyProviderType::StackitKms,
        }
    }
}

/// Settings serialize as the settings of their provider type, without a tag, as the provider type travels next to
/// them. While only one provider type has settings, they deserialize as that type's, so that a mistake names its field
/// rather than failing to match any variant. A second variant needs the provider type to tell the variants apart.
impl<'de> Deserialize<'de> for KeyProviderSettings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        StackitKmsSettings::deserialize(deserializer).map(Self::StackitKms)
    }
}

/// Where STACKIT KMS keeps the keys, a key ring in your STACKIT project, and the service account UniCore signs in as.
#[allow(
    clippy::doc_markdown,
    reason = "the docs are the API's schema descriptions, which name UniCore without backticks"
)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StackitKmsSettings {
    /// The ID of your STACKIT project.
    #[schema(example = "4a1d2f6e-0f33-4c55-9b56-6f8e8a2b1c3d")]
    pub project_id: String,
    pub region: StackitRegion,
    /// The ID of the key ring in that project that keeps the keys.
    #[schema(example = "9c0e7b52-5d2a-4f0e-8a51-3b6d1e2f4a7c")]
    pub key_ring_id: String,
    /// The service account UniCore signs in as, with access to the key ring. It must have a federated identity
    /// provider that trusts the workload identity this instance runs as, and that checks the subject as well as the
    /// issuer, since other workloads can share the issuer.
    #[schema(example = "unicore-kms@sa.stackit.cloud")]
    pub service_account_email: String,
}

/// The STACKIT regions that offer KMS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, ToSchema)]
pub enum StackitRegion {
    /// STACKIT's region `eu01`, the only one that offers KMS so far.
    #[serde(rename = "eu01")]
    #[strum(serialize = "eu01")]
    Eu01,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The settings of a STACKIT KMS provider whose keys are in the key ring `key_ring_id`.
    pub(crate) fn stackit(key_ring_id: &str) -> KeyProviderSettings {
        KeyProviderSettings::StackitKms(StackitKmsSettings {
            project_id: "4a1d2f6e-0f33-4c55-9b56-6f8e8a2b1c3d".to_string(),
            region: StackitRegion::Eu01,
            key_ring_id: key_ring_id.to_string(),
            service_account_email: "unicore-kms@sa.stackit.cloud".to_string(),
        })
    }
}

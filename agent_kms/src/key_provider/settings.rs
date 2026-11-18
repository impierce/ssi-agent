//! The settings of key providers that aren't secret, such as where in the provider the keys are kept.

use crate::key_provider::aggregate::KeyProviderType;
use serde::{Deserialize, Serialize};
use strum::Display;
use uuid::Uuid;

/// The settings of a key provider, for the provider types that have any.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyProviderSettings {
    StackitKms(StackitKmsSettings),
}

impl KeyProviderSettings {
    /// The type of provider these settings are for.
    #[must_use]
    pub fn provider_type(&self) -> KeyProviderType {
        match self {
            Self::StackitKms(_) => KeyProviderType::StackitKms,
        }
    }
}

/// Where STACKIT KMS keeps the keys, a key ring in the customer's project, and the service account `UniCore` acts as
/// there.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StackitKmsSettings {
    pub project_id: Uuid,
    pub region: StackitRegion,
    pub key_ring_id: Uuid,
    /// The service account `UniCore` signs in as. It must trust the workload identity `UniCore` runs as.
    pub service_account_email: String,
}

/// The STACKIT regions that offer KMS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, utoipa::ToSchema)]
pub enum StackitRegion {
    #[serde(rename = "eu01")]
    #[strum(serialize = "eu01")]
    Eu01,
}

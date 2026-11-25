use agent_shared::config::RefreshServiceConfiguration;
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use serde::{Deserialize, Serialize};

/// A credential configuration this issuer supports: the OpenID4VCI object it advertises, and UniCore's options for
/// the credentials it issues with it.
///
/// Serialized as the OpenID4VCI object with `refreshService` beside its fields, and without it when there is none,
/// so configurations stored before refresh existed read back unchanged. Only [`Self::metadata`] is advertised in
/// the issuer metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportedCredentialConfiguration {
    #[serde(flatten)]
    pub metadata: CredentialConfigurationsSupportedObject,
    /// Opts the configuration's credentials into credential refresh.
    #[serde(rename = "refreshService", default, skip_serializing_if = "Option::is_none")]
    pub refresh_service: Option<RefreshServiceConfiguration>,
}

impl From<CredentialConfigurationsSupportedObject> for SupportedCredentialConfiguration {
    /// A configuration without refresh.
    fn from(metadata: CredentialConfigurationsSupportedObject) -> Self {
        Self {
            metadata,
            refresh_service: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_configuration_without_refresh_keeps_its_stored_shape() {
        let stored = json!({"format": "dc+sd-jwt", "vct": "https://example.org/vct", "scope": "pid"});

        let configuration: SupportedCredentialConfiguration = serde_json::from_value(stored.clone()).unwrap();

        assert_eq!(serde_json::to_value(configuration).unwrap(), stored);
    }
}

use oid4vci::credential_format_profiles::{CredentialFormats, WithParameters};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CredentialConfiguration {
    pub credential_configuration_id: String,
    #[serde(flatten)]
    pub credential_format_with_parameters: CredentialFormats<WithParameters>,
    #[serde(default)]
    pub display: Vec<serde_json::Value>,
}

#[cfg(feature = "test_utils")]
use once_cell::sync::Lazy;

#[cfg(feature = "test_utils")]
pub static TEST_ISSUER_CONFIG: Lazy<serde_yaml::Value> = Lazy::new(|| {
    serde_yaml::from_str(
        r#"
            server_config:
              credential_configurations:
                - credential_configuration_id: badge
                  format: jwt_vc_json
                  credential_definition:
                    type:
                      - VerifiableCredential
                  display:
                    - name: Badge
                      logo:
                        url: https://example.com/logo.png
        "#,
    )
    .unwrap()
});

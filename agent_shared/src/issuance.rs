use oid4vci::credential_format_profiles::{CredentialFormats, WithParameters};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct CredentialConfiguration {
    pub credential_configuration_id: String,
    #[serde(flatten)]
    pub credential_format_with_parameters: CredentialFormats<WithParameters>,
    #[serde(default)]
    pub display: Vec<serde_json::Value>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ServerConfig {
    pub credential_configurations: Vec<CredentialConfiguration>,
}

pub static TEST_ISSUER_CONFIG: std::sync::Mutex<Option<serde_yaml::Value>> = std::sync::Mutex::new(None);

pub fn set_issuer_configuration() {
    // Set the test configuration.
    TEST_ISSUER_CONFIG.lock().unwrap().replace(
        serde_yaml::from_str(&format!(
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
            "#
        ))
        .unwrap(),
    );
}

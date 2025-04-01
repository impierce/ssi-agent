use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use oid4vci::credential_format_profiles::{
    w3c_verifiable_credentials::{
        jwt_vc_json::{CredentialDefinition, JwtVcJson, JwtVcJsonParameters},
        CredentialSubject,
    },
    CredentialFormats, Parameters,
};
use rand::Rng;
use std::str::FromStr;
use url::Url;

use crate::config::{
    ApplicationConfiguration, CredentialConfiguration, Display, EventStoreType, Logo, SupportedDidMethod, ToggleOptions,
};

impl ApplicationConfiguration {
    pub fn apply_development_defaults(&mut self) -> Self {
        self.event_store.type_ = EventStoreType::InMemory;

        // If no Stronghold password is provided, a random password is generated.
        let random_bytes: [u8; 16] = rand::thread_rng().gen();
        self.secret_manager.stronghold_password = Some(URL_SAFE_NO_PAD.encode(&random_bytes));
        println!(
            "\n====================\n\n  A new Stronghold password was generated!\n\n  {}\n\n====================\n",
            self.secret_manager.stronghold_password.clone().unwrap()
        );

        self.url = Some(Url::parse("http://localhost:3033").unwrap());
        self.did_methods.insert(
            SupportedDidMethod::Jwk,
            ToggleOptions {
                enabled: true,
                preferred: Some(true),
            },
        );
        self.did_methods.insert(
            SupportedDidMethod::Key,
            ToggleOptions {
                enabled: true,
                preferred: None,
            },
        );
        self.display.push(Display {
            name: "UniCore".to_string(),
            locale: Some("en".to_string()),
            logo: Some(Logo {
                uri: Some(Url::from_str("https://www.impierce.com/external/impierce-icon.png").unwrap()),
                alt_text: Some("Impierce Icon".to_string()),
            }),
        });
        self.credential_configurations.push(CredentialConfiguration {
            credential_configuration_id: "001".to_string(),
            credential_format_with_parameters: CredentialFormats::JwtVcJson(Parameters::<JwtVcJson> {
                parameters: JwtVcJsonParameters {
                    credential_definition: CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string()],
                        credential_subject: CredentialSubject::default(),
                    },
                    order: None,
                },
            }),
            display: vec![serde_json::to_value(Display {
                name: "Verifiable Credential".to_string(),
                locale: Some("en".to_string()),
                logo: Some(Logo {
                    uri: Some(Url::from_str("https://www.impierce.com/external/impierce-logo.png").unwrap()),
                    alt_text: Some("Impierce Logo".to_string()),
                }),
            })
            .unwrap()],
        });

        self.clone()
    }

    pub fn apply_production_defaults(&mut self) -> Self {
        self.domain_linkage_enabled = true;

        self.did_methods.insert(
            SupportedDidMethod::Jwk,
            ToggleOptions {
                enabled: false,
                preferred: None,
            },
        );
        self.did_methods.insert(
            SupportedDidMethod::Key,
            ToggleOptions {
                enabled: false,
                preferred: None,
            },
        );
        self.did_methods.insert(
            SupportedDidMethod::Web,
            ToggleOptions {
                enabled: true,
                preferred: Some(true),
            },
        );

        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_development_config() {}

    #[test]
    fn test_production_config() {}
}

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use oid4vci::credential_format_profiles::{
    w3c_verifiable_credentials::{
        jwt_vc_json::{CredentialDefinition, JwtVcJson, JwtVcJsonParameters},
        CredentialSubject,
    },
    CredentialFormats, Parameters,
};
use rand::Rng;
use url::Url;

use crate::config::{
    ApplicationConfiguration, CredentialConfiguration, Display, EventStoreType, LogFormat, SupportedDidMethod,
    ToggleOptions,
};

pub(crate) fn apply_development_defaults(mut config: ApplicationConfiguration) -> ApplicationConfiguration {
    config.event_store.type_ = EventStoreType::InMemory;

    // If no Stronghold password is provided, a random password is generated.
    let random_bytes: [u8; 16] = rand::thread_rng().gen();
    config.secret_manager.stronghold_password = Some(URL_SAFE_NO_PAD.encode(&random_bytes));
    println!(
        "\n====================\n\n  A new Stronghold password was generated!\n\n  {}\n\n====================\n",
        config.secret_manager.stronghold_password.clone().unwrap()
    );

    config.url = Some(Url::parse("http://localhost:3033").unwrap());
    config.did_methods.insert(
        SupportedDidMethod::Jwk,
        ToggleOptions {
            enabled: true,
            preferred: Some(true),
        },
    );
    config.did_methods.insert(
        SupportedDidMethod::Key,
        ToggleOptions {
            enabled: true,
            preferred: None,
        },
    );
    config.display.push(Display {
        name: "UniCore".to_string(),
        locale: None,
        logo: None,
    });
    config.credential_configurations.push(CredentialConfiguration {
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
            name: "My Verifiable Credential".to_string(),
            locale: None,
            logo: None,
        })
        .unwrap()],
    });

    config
}

pub(crate) fn apply_production_defaults(mut config: ApplicationConfiguration) -> ApplicationConfiguration {
    config.domain_linkage_enabled = true;

    config.did_methods.insert(
        SupportedDidMethod::Jwk,
        ToggleOptions {
            enabled: false,
            preferred: None,
        },
    );
    config.did_methods.insert(
        SupportedDidMethod::Key,
        ToggleOptions {
            enabled: false,
            preferred: None,
        },
    );
    config.did_methods.insert(
        SupportedDidMethod::Web,
        ToggleOptions {
            enabled: true,
            preferred: Some(true),
        },
    );

    config
}

/// Checks if the application configuration follows production-ready restrictions.
pub(crate) fn check_production_readiness(config: ApplicationConfiguration) {
    if config.secret_manager.stronghold_password.is_none()
        || config.secret_manager.stronghold_password.as_ref().unwrap().is_empty()
    {
        panic!("Stronghold password must be provided.");
    }
    // TODO: check password policy ...
    // Disallow `in_memory` in production?
}

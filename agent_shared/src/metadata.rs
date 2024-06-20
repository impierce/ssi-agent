use std::collections::HashMap;

use jsonwebtoken::Algorithm;
use oid4vc_core::SubjectSyntaxType;
use oid4vp::ClaimFormatDesignation;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tracing::info;
use url::Url;

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Logo {
    // TODO: remove this alias and change field to `uri`.
    #[serde(alias = "uri")]
    pub url: Option<Url>,
    pub alt_text: Option<String>,
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Display {
    pub name: Option<String>,
    pub locale: Option<String>,
    pub logo: Option<Logo>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Metadata {
    #[serde(default)]
    pub subject_syntax_types_supported: Vec<SubjectSyntaxType>,
    #[serde(default)]
    pub signing_algorithms_supported: Vec<Algorithm>,
    #[serde(default)]
    pub id_token_signing_alg_values_supported: Vec<Algorithm>,
    #[serde(default)]
    pub request_object_signing_alg_values_supported: Vec<Algorithm>,
    #[serde(default)]
    pub vp_formats: HashMap<ClaimFormatDesignation, serde_yaml::Mapping>,
    #[serde(default)]
    pub display: Vec<Display>,
}

#[cfg(feature = "test")]
pub static TEST_METADATA: std::sync::Mutex<Option<serde_yaml::Value>> = std::sync::Mutex::new(None);

pub fn load_metadata() -> Metadata {
    #[cfg(feature = "test")]
    let mut config = TEST_METADATA.lock().unwrap().as_ref().unwrap().clone();
    #[cfg(not(feature = "test"))]
    let mut config: serde_yaml::Value = {
        match std::fs::File::open("agent_application/config.yml") {
            Ok(config_file) => serde_yaml::from_reader(config_file).unwrap(),
            // If the config file does not exist, return an empty config.
            Err(_) => serde_yaml::Value::Null,
        }
    };

    let supported_algorithms: serde_yaml::Value = vec!["EdDSA".to_string()].into();
    if config["signing_algorithms_supported"] != supported_algorithms {
        unimplemented!(
            "\n{}\nOnly the `EdDSA` signing algorithm is supported",
            serde_yaml::to_string(&config["signing_algorithms_supported"]).unwrap()
        )
    }

    config.apply_merge().unwrap();

    let metadata =
        serde_yaml::from_value::<Metadata>(config).expect("Invalid metadata in `agent_application/config.yml` file");

    info!("Loaded metadata: {:?}", metadata);

    metadata
}

#[cfg(feature = "test")]
pub fn set_metadata_configuration(default_did_method: &str) {
    // Set the test configuration.
    TEST_METADATA.lock().unwrap().replace(
        serde_yaml::from_str(&format!(
            r#"
                subject_syntax_types_supported:
                    - {default_did_method}
                    - did:key
                    - did:iota:rms
                    - did:jwk
                signing_algorithms_supported: &signing_algorithms_supported
                    - EdDSA
                id_token_signing_alg_values_supported: *signing_algorithms_supported
                request_object_signing_alg_values_supported: *signing_algorithms_supported
                vp_formats:
                    jwt_vc_json:
                        alg: *signing_algorithms_supported
                    jwt_vp_json:
                        alg: *signing_algorithms_supported
                display:
                    - name: UniCore
                      locale: en
                      logo:
                        uri: https://impierce.com/images/logo-blue.png
                        alt_text: UniCore Logo
            "#
        ))
        .unwrap(),
    );
}

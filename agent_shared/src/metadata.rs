use jsonwebtoken::Algorithm;
use oid4vc_core::SubjectSyntaxType;
use oid4vp::ClaimFormatDesignation;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;
// use tracing::info;
use url::Url;

// use crate::config::config;

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
    pub name: String,
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

#[cfg(feature = "test_utils")]
pub static TEST_METADATA: std::sync::Mutex<Option<serde_yaml::Value>> = std::sync::Mutex::new(None);

// fn load_metadata() -> Metadata {
//     let mut metadata = Metadata::default();

//     metadata.subject_syntax_types_supported = config()
//         .did_methods
//         .into_iter()
//         .filter(|(_, v)| v.enabled)
//         .map(|(k, _)| SubjectSyntaxType::from_str(&k.replace('_', ":")).unwrap())
//         .collect();

//     metadata.signing_algorithms_supported = config()
//         .signing_algorithms_supported
//         .iter()
//         .filter(|(_, v)| v.enabled)
//         .map(|(k, _)| k.clone())
//         .collect();

//     metadata
//         .id_token_signing_alg_values_supported
//         .clone_from(&metadata.signing_algorithms_supported);
//     metadata
//         .request_object_signing_alg_values_supported
//         .clone_from(&metadata.signing_algorithms_supported);

//     metadata.display.clone_from(&config().display);

//     info!("Loaded metadata: {:?}", metadata);

//     info!("{:?}", serde_json::to_string(&metadata).unwrap());

//     metadata
// }

#[cfg(feature = "test_utils")]
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

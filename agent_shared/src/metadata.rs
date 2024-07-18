use jsonwebtoken::Algorithm;
use oid4vc_core::SubjectSyntaxType;
use oid4vp::ClaimFormatDesignation;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;
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

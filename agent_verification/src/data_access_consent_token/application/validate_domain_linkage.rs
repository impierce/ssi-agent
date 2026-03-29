use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use did_manager::Resolver;
use identity_credential::domain_linkage::{DomainLinkageConfiguration, JwtDomainLinkageValidator};
use identity_iota::{
    core::{FromJson, ToJson},
    credential::JwtCredentialValidationOptions,
    document::CoreDocument,
    verification::{
        jwk::Jwk as IotaIdentityJwk,
        jws::{JwsVerifier, SignatureVerificationError, SignatureVerificationErrorKind, VerificationInput},
    },
};
use jsonwebtoken::{crypto::verify, jwk::Jwk as JsonWebTokenJwk, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::str::FromStr;
use url::Url;

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct ValidationResult {
    pub(crate) status: ValidationStatus,
    pub(crate) name: Option<String>,
    pub(crate) logo_uri: Option<url::Url>,
    pub(crate) issuance_date: Option<String>,
    pub(crate) message: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub enum ValidationStatus {
    Success,
    #[default]
    Failure,
    Unknown,
}

/// This `Verifier` uses `jsonwebtoken` under the hood to verify verification input.
pub struct Verifier;
impl JwsVerifier for Verifier {
    fn verify(&self, input: VerificationInput, public_key: &IotaIdentityJwk) -> Result<(), SignatureVerificationError> {
        use SignatureVerificationErrorKind::*;

        let algorithm =
            Algorithm::from_str(&input.alg.to_string()).map_err(|_| SignatureVerificationError::new(UnsupportedAlg))?;

        // Convert the `IotaIdentityJwk` first into a `JsonWebTokenJwk` and then into a `DecodingKey`.
        let decoding_key = public_key
            .to_json()
            .ok()
            .and_then(|public_key| JsonWebTokenJwk::from_json(&public_key).ok())
            .and_then(|jwk| DecodingKey::from_jwk(&jwk).ok())
            .ok_or(SignatureVerificationError::new(KeyDecodingFailure))?;

        let mut validation = Validation::new(algorithm);
        validation.validate_aud = false;
        validation.required_spec_claims.clear();

        match verify(
            &URL_SAFE_NO_PAD.encode(input.decoded_signature),
            &input.signing_input,
            &decoding_key,
            algorithm,
        ) {
            Ok(true) => Ok(()),
            Err(_) | Ok(false) => Err(SignatureVerificationError::new(
                // TODO: more fine-grained error handling?
                InvalidSignature,
            )),
        }
    }
}

/// https://wiki.iota.org/identity.rs/how-tos/domain-linkage/create-and-verify/#verifying-a-did-and-domain-linkage
pub async fn validate_domain_linkage(resolver: &Resolver, url: url::Url, did: &str) -> ValidationResult {
    let did_configuration_result = fetch_configuration(url.clone()).await;

    let domain_linkage_configuration = match did_configuration_result {
        Ok(did_config) => did_config,
        Err(err) => {
            return ValidationResult {
                status: ValidationStatus::Unknown,
                message: Some(format!("Error while fetching configuration: {err}")),
                ..Default::default()
            };
        }
    };

    let validator = JwtDomainLinkageValidator::with_signature_verifier(Verifier);

    let document = match resolver.resolve(did).await {
        Ok(document) => document,
        Err(e) => {
            return ValidationResult {
                status: ValidationStatus::Unknown,
                message: Some(e.to_string()),
                ..Default::default()
            };
        }
    };

    let url = identity_iota::core::Url::from(url);

    let res = validator.validate_linkage(
        &document,
        &domain_linkage_configuration,
        &url,
        &JwtCredentialValidationOptions::default(),
    );

    if res.is_ok() {
        ValidationResult {
            status: ValidationStatus::Success,
            ..Default::default()
        }
    } else {
        ValidationResult {
            status: ValidationStatus::Failure,
            message: res.err().map(|e| e.to_string()),
            ..Default::default()
        }
    }
}

/// Acts as a replacement for `fetch_configuration()` from `identity_credential` which fails on JSON-LD inside `linked_dids`.
/// This implementation is also less strict (allows `http` scheme, does not fail on JSON-LD)
/// The resource at the `.well-known` endpoint is fetched and any non-string values from `linked_dids` before deserializing.
/// Returns a `DomainLinkageConfiguration` which can be verified using a verifier from `identity_credential`.
async fn fetch_configuration(mut url: url::Url) -> Result<DomainLinkageConfiguration, String> {
    // 1. Prepare the URL
    url.set_fragment(None);
    url.set_query(None);
    url.set_path(".well-known/did-configuration.json");

    // 2. Fetch the resource
    let response = reqwest::get(url.clone())
        .await
        .map_err(|_| format!("failed to get response from resource url: {url}"))?;

    // 3. Parse to JSON value (mutable)
    let mut json = response
        .json::<serde_json::Value>()
        .await
        .map_err(|_| "failed to parse response into JSON value".to_string())?;

    // 4. Remove all non-string values from `linked_dids` (JSON-LD)
    if let serde_json::Value::Object(ref mut root) = json {
        if let Some(serde_json::Value::Array(ref mut linked_dids)) = root.get_mut("linked_dids") {
            linked_dids.retain(|did| matches!(did, serde_json::Value::String(_)));
        }
    }

    // 5. Deserialize to `DomainLinkageConfiguration`
    let config = DomainLinkageConfiguration::from_json_value(json)
        .map_err(|_| "failed to deserialize DomainLinkageConfiguration from JSON".to_string())?;
    Ok(config)
}

/// Get the linked domains from the issuer document. It returns a list of URLs if the service type is `LinkedDomains`.
pub async fn get_issuer_linked_domains(issuer_document: &CoreDocument) -> Vec<Url> {
    issuer_document
        .service()
        .iter()
        .filter_map(|service| {
            service
                .type_()
                .contains("LinkedDomains")
                .then(|| service.service_endpoint())
                .and_then(|service_endpoint| service_endpoint.to_json_value().ok())
                .and_then(|linked_domain| {
                    linked_domain.get("origins").and_then(|origins| {
                        origins.as_array().and_then(|origins| {
                            origins
                                .iter()
                                .map(|origin| {
                                    origin.as_str().and_then(|origin| {
                                        origin
                                            .parse()
                                            .inspect_err(|err| println!("Failed to parse linked domain: {err:#?}"))
                                            .ok()
                                    })
                                })
                                .collect::<Option<Vec<Url>>>()
                        })
                    })
                })
        })
        .flatten()
        .collect()
}

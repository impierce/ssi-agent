use crate::{
    data_access_consent_token::{
        application::{
            validate_domain_linkage::{get_issuer_linked_domains, validate_domain_linkage, ValidationStatus},
            validate_linked_verifiable_presentation::validate_linked_verifiable_presentations,
        },
        error::DataAccessConsentTokenError,
    },
    state::VerificationState,
};

use agent_shared::{
    convert_iota_jwk_to_decoding_key, credential_status_checker::CredentialStatusChecker, get_unverified_jwt_claims,
    handlers::query_handler,
};
use identity_iota::document::ServiceEndpoint;
use jsonwebtoken::{decode, decode_header, Validation};
use oid4vc_core::credential_status_verifier::CredentialStatusVerifier;
use serde::{Deserialize, Serialize};
use tracing::info;
use url::Url;

pub const DATA_ACCESS_ENDPOINT: &str = "DataAccessConsentTokenEndpoint";

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct RedeemDataAccessConsentTokenService {
    public_verification_response: PublicVerificationResponse, // this is used to build the response step by step as we perform the different validation steps
}

impl RedeemDataAccessConsentTokenService {
    /// TODO
    pub async fn validate_data_access_consent_token(
        mut self,
        token_id: String,
        state: &VerificationState,
    ) -> Result<(Url, String), DataAccessConsentTokenError> {
        // Data Access Consent Token will hereafter be referred to as DACT for brevity
        let data_access_consent_token = query_handler(&token_id, &state.query.data_access_consent_token)
            .await
            .map_err(|e| DataAccessConsentTokenError::QueryError(e.to_string()))?
            .ok_or(DataAccessConsentTokenError::DataAccessConsentTokenNotFound(
                token_id.clone(),
            ))?;

        // Get unverified claims
        let token_value = serde_json::Value::String(data_access_consent_token.token.clone());
        let dact_claims = get_unverified_jwt_claims(&token_value).ok_or(DataAccessConsentTokenError::DACTError(
            "Failed to get the unverified JWT claims".to_string(),
        ))?;

        // Extract the `aud` claim, it equals the issuer DID of the credential which is given access to.
        let aud = dact_claims
            .get("aud")
            .and_then(|v| v.as_str())
            .ok_or(DataAccessConsentTokenError::DACTError(
                "Failed to get `aud` claim from DACT".to_string(),
            ))?;

        let resolver = &state.subject.resolver;
        let issuer_did_document = resolver
            .resolve(aud)
            .await
            .map_err(|e| DataAccessConsentTokenError::DidResolutionError(e.to_string()))?;

        // Check and validate domain linkage

        info!("Issuer DID Document: {:#?}", issuer_did_document);

        let mut linked_domains = get_issuer_linked_domains(&issuer_did_document).await;
        for url in linked_domains.clone() {
            let validation_result = validate_domain_linkage(&resolver, url.clone(), aud).await;
            if validation_result.status == ValidationStatus::Success {
                self.public_verification_response.domain_linkage.push(ValidationResult {
                    status: ValidationStatus::Success,
                    payload: Some(url.to_string()),
                    data: None,
                });
            } else {
                linked_domains.retain(|u| u != &url);
            }
        }

        // Fallback for did:webs if no domain linkage is found
        if linked_domains.is_empty() {
            match aud.starts_with("did:web") {
                true => {
                    let did_web_domain =
                        extract_url_from_did_web(aud).ok_or(DataAccessConsentTokenError::DidResolutionError(
                            "Failed to extract URL from Issuer did:web".to_string(),
                        ))?;

                    info!("Extracted URL from did:web: {:#?}", did_web_domain);
                    self.public_verification_response.domain_linkage.push(ValidationResult {
                        status: ValidationStatus::Success,
                        payload: Some(did_web_domain.to_string()),
                        data: None,
                    });
                    linked_domains.push(did_web_domain);
                }
                false => {
                    self.public_verification_response.domain_linkage.push(ValidationResult {
                        status: ValidationStatus::Failure,
                        payload: Some("No linked domains found for issuer, and issuer is not a did:web".to_string()),
                        data: None,
                    });
                }
            }
        }

        info!("Linked Domains: {:#?}", linked_domains);

        // Get and validate the issuers linked verifiable presentations.
        let linked_verifiable_credentials: Vec<_> =
            validate_linked_verifiable_presentations(&resolver, &issuer_did_document)
                .await
                .into_iter()
                .flatten()
                .collect();

        match linked_verifiable_credentials.is_empty() {
            true => {
                self.public_verification_response.linked_vp.push(ValidationResult {
                    status: ValidationStatus::Failure,
                    // TODO: this is a hackathon specific message
                    payload: Some("No valid certifications found for the issuer".to_string()),
                    data: None,
                });
            }
            false => {
                for linked_vp in &linked_verifiable_credentials {
                    self.public_verification_response.linked_vp.push(ValidationResult {
                        status: ValidationStatus::Success,
                        // TODO: this is a hackathon specific message
                        payload: Some("Valid certifications found for the issuer".to_string()),
                        data: Some(serde_json::to_value(linked_vp).map_err(|_e| {
                            DataAccessConsentTokenError::DidResolutionError(
                                "TODO: this is an incorrect error message".to_string(),
                            )
                        })?), // TODO
                    });
                }
            }
        }

        info!("Linked Verifiable Credentials: {:#?}", linked_verifiable_credentials);

        // Validate status of Data Access Consent Token
        if let Some(status_claim) = dact_claims.get("status") {
            let credential_status_checker = CredentialStatusChecker {
                verification_material_resolver: state.subject.clone(),
            };

            credential_status_checker
                .check_credential_status(status_claim.to_owned())
                .await
                .map_err(|e| DataAccessConsentTokenError::DACTError(e.to_string()))?;
        }

        // Validate the signature of the Data Access Consent Token
        let jwt_header = decode_header(&data_access_consent_token.token).map_err(|e| {
            DataAccessConsentTokenError::DACTError(format!(
                "Failed to decode JWT header of the Data Access Consent Token: {e}"
            ))
        })?;

        let kid = jwt_header.kid.ok_or(DataAccessConsentTokenError::DACTError(
            "JWT header is missing `kid` field".to_string(),
        ))?;

        // Fetch the public key using the kid
        let public_key = state.subject.resolve_public_key(&kid).await.map_err(|_| {
            DataAccessConsentTokenError::DACTError("Failed to fetch public key for JWT verification".to_string())
        })?;

        let decoding_key =
            convert_iota_jwk_to_decoding_key(&public_key).ok_or(DataAccessConsentTokenError::DACTError(
                "Failed to convert public key into decoding key for JWT verification".to_string(),
            ))?;

        // TODO: should more validation parameters be set??
        let validation = Validation::new(jwt_header.alg);

        // Decode and verify the JWT signature
        decode::<serde_json::Value>(&data_access_consent_token.token, &decoding_key, &validation).map_err(|e| {
            DataAccessConsentTokenError::DACTError(format!(
                "JWT signature verification failed for the Data Access Consent Token: {e}"
            ))
        })?;

        // TODO: validate the trust relation.

        // TODO: All primary checks have passed for the Public Credential Token at this point, to perform the remaining checks we need to fetch the Public Credential from the Issuer.

        // Discover public credential endpoint through DID resolution
        let data_access_endpoint = issuer_did_document
            .service()
            .iter()
            .find(|service| service.type_().contains(DATA_ACCESS_ENDPOINT))
            .and_then(|service| match service.service_endpoint() {
                ServiceEndpoint::One(url) => Some(url.clone()),
                // TODO: handle multiple endpoints?
                ServiceEndpoint::Set(urls) => urls.first().cloned(),
                ServiceEndpoint::Map(map) => map.values().next().and_then(|urls| urls.first().cloned()),
            })
            .ok_or(DataAccessConsentTokenError::NoDataAccessEndpointFound(
                "No Data Access Endpoint found in the Issuer DID Document services".to_string(),
            ))?;

        let data_access_endpoint_url = Url::parse(&data_access_endpoint.to_string()).map_err(|_| {
            DataAccessConsentTokenError::NoDataAccessEndpointFound(
                "Failed to parse Data Access Endpoint into URL".to_string(),
            )
        })?;

        Ok((data_access_endpoint_url, data_access_consent_token.token))
    }

    /// TODO
    pub async fn validate_data_access_endpoint_response(
        mut self,
        data_access_consent_token: String,
        response: DataAccessEndpointResponse,
        state: &VerificationState,
    ) -> Result<(), DataAccessConsentTokenError> {
        let verifiable_credential_claims =
            get_unverified_jwt_claims(&serde_json::Value::String(response.verifiable_credential.clone())).ok_or(
                DataAccessConsentTokenError::InvalidResponse("Failed to get response JWT claims".to_string()),
            )?;
        let data_access_consent_token_claims =
            get_unverified_jwt_claims(&serde_json::Value::String(data_access_consent_token.clone())).ok_or(
                DataAccessConsentTokenError::DACTError("Failed to get token JWT claims".to_string()),
            )?;

        // The subject of the Public Credential Token is the JTI (credential ID) of the issued credential which the Verifier is trying to access
        let sub = data_access_consent_token_claims
            .get("sub")
            .and_then(|v| v.as_str())
            .ok_or(DataAccessConsentTokenError::DACTError(
                "Failed to get `sub` claim from Public Credential Token".to_string(),
            ))?;

        let jti = verifiable_credential_claims.get("jti").and_then(|v| v.as_str()).ok_or(
            DataAccessConsentTokenError::InvalidResponse(
                "Failed to get `jti` claim from Public Credential".to_string(),
            ),
        )?;

        if sub != jti {
            // This would equal StatusCode::UNPROCESSABLE_ENTITY 422.
            return Err(DataAccessConsentTokenError::InvalidResponse(
                "The `sub` claim of the Data Access Consent Token does not match the `jti` claim of the issued credential".to_string(),
            ));
        }

        // TODO: how to combine the basic Err flow of this function with the public_verification_response building in the best way??
        // TODO: none of this works with sd-jwt yet

        // TODO checking the iss and the kid both against the credentialSubject.id seems a bit duplicate, it should be the kid since anyone can enter a random iss value but the kid will also be checked for the signature.
        // Extract credential subject ID from response VC
        let credential_subject_id = verifiable_credential_claims.get("vc")
            .and_then(|data| data.get("credentialSubject"))
            .and_then(|cred_subject| cred_subject.get("id"))
            .and_then(|id| id.as_str())
            .ok_or(
                DataAccessConsentTokenError::InvalidResponse(
                    "Requested credential is missing the credentialSubject.id field. Publicly sharing anonymous credentials is not supported.".to_string(),
                )
            )?;

        // Extract the `iss` claim from the Data Access Consent Token
        let iss = data_access_consent_token_claims
            .get("iss")
            .and_then(|v| v.as_str())
            .ok_or(DataAccessConsentTokenError::DACTError(
                "Failed to get `iss` claim from Data Access Consent Token".to_string(),
            ))?;

        // Check whether the issuer of the Data Access Consent Token matches the subject of the received credential
        // This check means we currently don't allow to publicly share anonymous credentials
        if iss != credential_subject_id {
            // This would equal StatusCode::UNPROCESSABLE_ENTITY 422.
            return Err(DataAccessConsentTokenError::InvalidResponse(
                "The `iss` claim of the Data Access Consent Token does not match the `id` claim of the Credential Subject".to_string(),
            ));
        }

        // Validate credential status claim
        if let Some(status_claim) = verifiable_credential_claims.get("status") {
            let credential_status_checker = CredentialStatusChecker {
                verification_material_resolver: state.subject.clone(),
            };

            let status = credential_status_checker
                .check_credential_status(status_claim.to_owned())
                .await;

            match status {
                Ok(_) => {
                    self.public_verification_response.credential_status.status = ValidationStatus::Success;
                }
                Err(_) => {
                    self.public_verification_response.credential_status.status = ValidationStatus::Failure;
                    self.public_verification_response.credential_status.payload =
                        Some(format!("The credential status is invalid"));
                }
            }
        }

        // Decode header to get kid
        let jwt_header = decode_header(&data_access_consent_token).map_err(|e| {
            DataAccessConsentTokenError::InvalidResponse(format!(
                "Failed to decode JWT header of the received credential: {e}"
            ))
        })?;

        let kid = jwt_header.kid.ok_or(DataAccessConsentTokenError::InvalidResponse(
            "JWT header is missing `kid` field".to_string(),
        ))?;

        // Validate the kid belongs to the same DID as credential subject
        let kid_did = kid.split('#').next().unwrap_or(&kid);
        if kid_did != credential_subject_id {
            return Err(DataAccessConsentTokenError::InvalidResponse(
                "Data Access Consent Token kid does not match requested credential subject DID".to_string(),
            ));
        }

        // TODO this seems like Data Access Consent Token validation and it should go there
        // Fetch the public key using the kid
        let public_key = state.subject.resolve_public_key(&kid).await.map_err(|_| {
            DataAccessConsentTokenError::InvalidResponse("Failed to fetch public key for JWT verification".to_string())
        })?;

        let decoding_key =
            convert_iota_jwk_to_decoding_key(&public_key).ok_or(DataAccessConsentTokenError::InvalidResponse(
                "Failed to convert public key into decoding key for JWT verification".to_string(),
            ))?;

        // TODO: more validation parameters should be set
        let mut validation = Validation::new(jwt_header.alg);
        validation.set_issuer(&[credential_subject_id]);
        validation.sub = Some(sub.to_string());
        // validation.set_audience(&[aud]);

        // Decode and verify the JWT signature
        decode::<serde_json::Value>(&response.verifiable_credential, &decoding_key, &validation).map_err(|e| {
            DataAccessConsentTokenError::InvalidResponse(format!(
                "JWT signature verification failed for the received credential: {e}"
            ))
        })?;
        self.public_verification_response.proof.status = ValidationStatus::Success;

        // If all validations have passed, set the credential in the response
        if self.public_verification_response.proof.status == ValidationStatus::Success
            && self.public_verification_response.credential_status.status == ValidationStatus::Success
            && self.public_verification_response.trust_relation.status == ValidationStatus::Success
            && self.public_verification_response.linked_vp[0].status == ValidationStatus::Success // TODO: Fix this hard indexing
            && self.public_verification_response.domain_linkage[0].status == ValidationStatus::Success
        // TODO: Fix this hard indexing
        {
            let credential_data =
                verifiable_credential_claims
                    .get("vc")
                    .cloned()
                    .ok_or(DataAccessConsentTokenError::InvalidResponse(
                        "Failed to extract credential data from the response".to_string(),
                    ))?;
            self.public_verification_response.credential = Some(credential_data);
        }

        Ok(())
    }
}

// TODO: should we enable access tokens for multiple credentials? then this should be a vec
pub struct DataAccessEndpointResponse {
    pub verifiable_credential: String,
}

// TODO: review if we still want these response structs or not
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct ValidationResult {
    status: ValidationStatus,
    payload: Option<String>,
    data: Option<serde_json::Value>,
}

// TODO: make stronger typing then strings
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct PublicVerificationResponse {
    pub credential: Option<serde_json::Value>,
    pub proof: ValidationResult,
    pub credential_status: ValidationResult,
    pub trust_relation: ValidationResult,
    pub linked_vp: Vec<ValidationResult>,
    pub domain_linkage: Vec<ValidationResult>,
}

// Helpers

fn extract_url_from_did_web(did_web: &str) -> Option<Url> {
    if let Some(did) = did_web.strip_prefix("did:web:") {
        let url_str = if let Some(index_colon) = did.find(':') {
            &did[..index_colon]
        } else {
            did
        };

        // TODO: quick hack to solve the percent-encoding issue in did:web:localhost%3A3033 (localhost:3033)
        let url_decoded = url_str.replace("%3A", ":");

        if let Ok(url) = Url::parse(&format!("https://{url_decoded}")) {
            return Some(url);
        }
    }
    None
}

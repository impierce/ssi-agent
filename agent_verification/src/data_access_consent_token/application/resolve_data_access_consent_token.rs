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
    config::config, convert_iota_jwk_to_decoding_key, credential_status_checker::CredentialStatusChecker,
    get_unverified_jwt_claims, handlers::query_handler,
};
use identity_iota::document::ServiceEndpoint;
use jsonwebtoken::{decode, decode_header, Validation};
use oid4vc_core::credential_status_verifier::CredentialStatusVerifier;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::info;
use url::Url;

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct ResolveDataAccessConsentTokenService {
    pub dact_id: String,
    public_verification_response: PublicVerificationResponse,
    // empty strings will fail at any and every step in the flow, so no need to wrap them in an Option
    data_access_endpoint: String,
    dact: String,
    dact_did: String,
    consented_credential: String,
}

impl ResolveDataAccessConsentTokenService {
    // When receiving a request to store a new Data Access Consent Token, the token will be in the payload and therefore set in the `dact` field. When retrieving one from the event store, only the ID is needed.
    pub fn new(dact_id: String, dact: Option<String>) -> Self {
        Self {
            dact_id,
            dact: dact.unwrap_or_default(), // Will default to an empty string.
            ..Default::default()
        }
    }

    pub async fn validate_before_storage(
        &mut self,
        state: &VerificationState,
    ) -> Result<(), DataAccessConsentTokenError> {
        // This means we currently only accept DACTs for Verifiable Link purposes.
        if config().public_verification_endpoint_enabled {
            // TODO: write without else blocks
            // Check if the DACT already exists before going through the full resolve flow to avoid unnecessary workload.
            if query_handler(&self.dact_id, &state.query.data_access_consent_token)
                .await
                .map_err(|e| DataAccessConsentTokenError::QueryError(e.to_string()))?
                .is_some()
            {
                return Err(DataAccessConsentTokenError::DACTAlreadyExists(self.dact_id.clone()));
            } else {
                self.resolve_data_access_consent_token(state).await?;
            }
        } else {
            return Err(DataAccessConsentTokenError::EndpointNotEnabled);
        }

        Ok(())
    }

    /// This function performs all the necessary steps to resolve and validate both the Data Access Consent Token and the response from the Issuer's Data Access Endpoint, and returns a PublicVerificationResponse which contains the results of all the performed checks and validations along with the requested credential when all checks have passed.
    pub async fn resolve_data_access_consent_token(
        &mut self,
        state: &VerificationState,
    ) -> Result<PublicVerificationResponse, DataAccessConsentTokenError> {
        // Data Access Consent Token will hereafter be referred to as DACT for brevity
        if self.dact.is_empty() {
            let data_access_consent_token = query_handler(&self.dact_id, &state.query.data_access_consent_token)
                .await
                .map_err(|e| DataAccessConsentTokenError::QueryError(e.to_string()))?
                .ok_or(DataAccessConsentTokenError::DataAccessConsentTokenNotFound(
                    self.dact_id.clone(),
                ))?;

            self.dact = data_access_consent_token.token;
        }

        self.validate_data_access_consent_token(state).await?;
        self.fetch_consented_credential().await?;
        self.validate_data_access_endpoint_response(state).await?;

        Ok(self.public_verification_response.clone())
    }

    /// This function validates the Data Access Consent Token by performing the following steps:
    /// 1. Validate the domain linkage for the issuer of the DACT
    /// 2. Validate the linked verifiable credentials for the issuer of the DACT
    /// 3. Validate the status of the DACT if it has a status claim
    /// 4. Validate the signature of the DACT
    async fn validate_data_access_consent_token(
        &mut self,
        state: &VerificationState,
    ) -> Result<(), DataAccessConsentTokenError> {
        // Get unverified claims
        let token_value = serde_json::Value::String(self.dact.clone());
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
        let aud_did_document = resolver
            .resolve(aud)
            .await
            .map_err(|e| DataAccessConsentTokenError::DidResolutionError(e.to_string()))?;

        // Check and validate domain linkage

        info!("Issuer DID Document: {:#?}", aud_did_document);

        let mut linked_domains = get_issuer_linked_domains(&aud_did_document).await;
        for url in linked_domains.clone() {
            let validation_result = validate_domain_linkage(resolver, url.clone(), aud).await;
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
            validate_linked_verifiable_presentations(resolver, &aud_did_document)
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
                            DataAccessConsentTokenError::DACTError(
                                "Failed to serialize linked verifiable credential".to_string(),
                            )
                        })?), // TODO: should this really be an error or just a None?
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
        let jwt_header = decode_header(&self.dact).map_err(|e| {
            DataAccessConsentTokenError::DACTError(format!(
                "Failed to decode JWT header of the Data Access Consent Token: {e}"
            ))
        })?;

        let kid = jwt_header.kid.ok_or(DataAccessConsentTokenError::DACTError(
            "JWT header is missing `kid` field".to_string(),
        ))?;

        // Save dact_did for later validation.
        let dact_did = kid.split('#').next().unwrap_or(&kid);
        self.dact_did = dact_did.to_string();

        // Fetch the public key using the kid
        let public_key = state.subject.resolve_public_key(&kid).await.map_err(|_| {
            DataAccessConsentTokenError::DACTError("Failed to fetch public key for JWT verification".to_string())
        })?;

        let decoding_key =
            convert_iota_jwk_to_decoding_key(&public_key).ok_or(DataAccessConsentTokenError::DACTError(
                "Failed to convert public key into decoding key for JWT verification".to_string(),
            ))?;

        // TODO: should more validation parameters be set??
        let mut validation = Validation::new(jwt_header.alg);
        validation.validate_aud = false;

        // Decode and verify the JWT signature
        decode::<serde_json::Value>(&self.dact, &decoding_key, &validation).map_err(|e| {
            DataAccessConsentTokenError::DACTError(format!(
                "JWT signature verification failed for the Data Access Consent Token: {e}"
            ))
        })?;

        // TODO: validate the trust relation.

        // TODO: All primary checks have passed for the Data Access Consent Token at this point, to perform the remaining checks we need to fetch the Public Credential from the Issuer.

        // Discover Data Access endpoint through DID resolution
        let data_access_endpoint = aud_did_document
            .service()
            .iter()
            .find(|service| service.type_().contains("data-access-service")) // This str equals const DATA_ACCESS_SERVICE_ID in `agent_identity/src/state.rs`
            .and_then(|service| match service.service_endpoint() {
                ServiceEndpoint::One(url) => Some(url.clone()),
                // TODO: handle multiple endpoints?
                ServiceEndpoint::Set(urls) => urls.first().cloned(),
                ServiceEndpoint::Map(map) => map.values().next().and_then(|urls| urls.first().cloned()),
            })
            .ok_or(DataAccessConsentTokenError::NoDataAccessEndpointFound(
                "No Data Access Endpoint found in the Issuer DID Document services".to_string(),
            ))?;

        self.data_access_endpoint = data_access_endpoint.to_string();

        Ok(())
    }

    /// This function sends the Data Access Consent Token to the Issuer's Data Access Endpoint and expects to receive the requested credential in the response.
    async fn fetch_consented_credential(&mut self) -> Result<(), DataAccessConsentTokenError> {
        let request_body = DataAccessRequest {
            data_access_consent_token: self.dact.clone(),
        };

        info!(
            "Posting Data Access Consent Token to Data Access EndPoint: {}",
            self.data_access_endpoint
        );

        let response = reqwest::Client::new()
            .post(&self.data_access_endpoint)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| DataAccessConsentTokenError::DataAccessEndpointFetchError(e.to_string()))?;

        info!("Response: {response:?}");

        let status = response.status();
        if status != StatusCode::OK {
            return Err(DataAccessConsentTokenError::DataAccessEndpointFetchError(
                status.to_string(),
            ));
        }

        let typed_response: DataAccessEndpointResponse =
            response.json::<DataAccessEndpointResponse>().await.map_err(|e| {
                DataAccessConsentTokenError::InvalidResponse(format!(
                    "Failed to parse response from Issuer Data Access endpoint: {e}"
                ))
            })?;

        self.consented_credential = typed_response.verifiable_credential.clone();

        Ok(())
    }

    /// This function validates the response from the Issuer's Data Access Endpoint by performing the following steps:
    /// 1. Validate that the `sub` claim of the DACT matches the `jti` claim of the received credential
    /// 2. Validate that the did in the `kid` claim of the DACT matches the `id` claim of the Credential Subject in the received credential
    /// 3. Validate the credential status of the received credential if it has a status claim
    /// 4. Validate that the DID in the `kid` of the received credential matches the `aud` claim of the DACT
    /// 5. Validate the signature of the received credential using the `kid` in the DACT header to fetch the correct public key for verification
    ///
    /// If all validations pass, the received credential is set in the `credential` field of the `PublicVerificationResponse`.
    /// If any validation fails, the `PublicVerificationResponse` will still contain the results of all performed checks and validations along with the reasons for any failures, but the `credential` field will be None.
    async fn validate_data_access_endpoint_response(
        &mut self,
        state: &VerificationState,
    ) -> Result<PublicVerificationResponse, DataAccessConsentTokenError> {
        let verifiable_credential_claims =
            get_unverified_jwt_claims(&serde_json::Value::String(self.consented_credential.clone())).ok_or(
                DataAccessConsentTokenError::InvalidResponse("Failed to get response JWT claims".to_string()),
            )?;
        let dact_claims = get_unverified_jwt_claims(&serde_json::Value::String(self.dact.clone())).ok_or(
            DataAccessConsentTokenError::DACTError("Failed to get token JWT claims".to_string()),
        )?;

        // The subject of the Public Credential Token is the JTI (credential ID) of the issued credential which the Verifier is trying to access
        let sub = dact_claims
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
                        Some("The credential status is invalid".to_string());
                }
            }
        }

        // TODO: how to combine the basic Err flow of this function with the public_verification_response building in the best way??
        // TODO: none of this works with sd-jwt yet

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

        // Validate the DACT did belongs to the same DID as credential subject
        if self.dact_did != credential_subject_id {
            return Err(DataAccessConsentTokenError::InvalidResponse(
                "Data Access Consent Token DID does not match requested credential subject DID".to_string(),
            ));
        }
        // Decode Consented Credential header to get kid
        let vc_jwt_header = decode_header(&self.consented_credential).map_err(|e| {
            DataAccessConsentTokenError::InvalidResponse(format!(
                "Failed to decode JWT header of the received credential: {e}"
            ))
        })?;

        let vc_kid = vc_jwt_header.kid.ok_or(DataAccessConsentTokenError::InvalidResponse(
            "JWT header is missing `kid` field".to_string(),
        ))?;

        // Validate the Consented Credential DID belongs to the same DID as in the DACT `aud` claim.
        let vc_did = vc_kid.split('#').next().unwrap_or(&vc_kid);
        if vc_did
            != dact_claims
                .get("aud")
                .and_then(|v| v.as_str())
                .ok_or(DataAccessConsentTokenError::DACTError(
                    "Failed to get `aud` claim from DACT".to_string(),
                ))?
        {
            return Err(DataAccessConsentTokenError::InvalidResponse("The DID in the `kid` in the received credential does not match the `aud` claim of the Data Access Consent Token".to_string()));
        }

        // Fetch the public key using the kid
        let public_key = state.subject.resolve_public_key(&vc_kid).await.map_err(|_| {
            DataAccessConsentTokenError::InvalidResponse("Failed to fetch public key for JWT verification".to_string())
        })?;

        let decoding_key =
            convert_iota_jwk_to_decoding_key(&public_key).ok_or(DataAccessConsentTokenError::InvalidResponse(
                "Failed to convert public key into decoding key for JWT verification".to_string(),
            ))?;

        // TODO: more validation parameters should be set
        let mut validation = Validation::new(vc_jwt_header.alg);
        // validation.set_issuer(&[credential_subject_id]);

        // validation.sub = Some(sub.to_string());
        // validation.set_audience(&[aud]);

        // Decode and verify the JWT signature
        decode::<serde_json::Value>(&self.consented_credential, &decoding_key, &validation).map_err(|e| {
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

        Ok(self.public_verification_response.clone())
    }
}

// TODO: Enable access tokens for multiple VC's
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct DataAccessEndpointResponse {
    pub verifiable_credential: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct ValidationResult {
    status: ValidationStatus,
    payload: Option<String>,
    data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct PublicVerificationResponse {
    pub credential: Option<serde_json::Value>,
    pub proof: ValidationResult,
    pub credential_status: ValidationResult,
    pub trust_relation: ValidationResult,
    pub linked_vp: Vec<ValidationResult>,
    pub domain_linkage: Vec<ValidationResult>,
}

#[derive(Serialize, Deserialize)]
pub struct DataAccessRequest {
    #[serde(rename = "data-access-consent-token")]
    pub data_access_consent_token: String,
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

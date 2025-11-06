use agent_holder::credential::aggregate::get_unverified_jwt_claims;
use agent_secret_manager::subject::get_public_key_from_kid;
use agent_shared::config::config;
use agent_verification::state::VerificationState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use did_manager::Resolver;
use http_api_problem::ApiError;
use identity_iota::document::ServiceEndpoint;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use crate::verification::{
    validate_domain_linkage::{get_issuer_linked_domains, validate_domain_linkage, ValidationStatus},
    validate_linked_verifiable_presentation::validate_linked_verifiable_presentations,
};

#[derive(Deserialize)]
pub struct PublicVerificationQuery {
    #[serde(rename = "public-credential-token")]
    public_credential_token: String,
}

#[derive(Serialize, Default)]
pub struct ValidationResult {
    status: ValidationStatus,
    payload: Option<String>,
}

// TODO: make stronger typing then strings
#[derive(Serialize, Default)]
pub struct PublicVerificationResponse {
    pub credential: Option<serde_json::Value>,
    pub proof: String,
    pub status: String,
    pub trust_relation: String,
    pub linked_vp: String,
    pub domain_linkage: ValidationResult,
}

/// This endpoint receives a Public Credential Token as a query parameter and then performs several validation steps on the token.
/// When all validations pass, the requested credential is returned in the response along with the validation results.
/// When any validation fails, only the validation results are returned.
/// Both the Verifier and the Issuer need to perform all these checks on the Public Credential Token, zero trust is assumed.
pub async fn public_verification(
    State(state): State<VerificationState>,
    Query(parameter): Query<PublicVerificationQuery>,
) -> Result<Response, ApiError> {
    let jwt = parameter.public_credential_token;
    let mut public_verification_response = PublicVerificationResponse::default();

    // Get unverified claims
    let jwt_value = serde_json::Value::String(jwt.clone());
    let public_credential_token_claims = get_unverified_jwt_claims(&jwt_value).map_err(|_| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid JWT")
            .message("Failed to decode Public Credential Token")
            .finish()
    })?;

    // Extract the `aud` claim
    let aud = public_credential_token_claims
        .get("aud")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Token")
                .message("Failed to get `aud` claim from Public Credential Token")
                .finish()
        })?;

    // Check and validate domain linkage
    let resolver = Resolver::new().await;
    let issuer_did_document = resolver
        .resolve(aud)
        .await
        .inspect_err(|err| println!("Failed to resolve issuer DID.: {err:#?}"))
        .map_err(|_| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Token")
                .message("Failed to resolve issuer DID")
                .finish()
        })?;

    let linked_domains = get_issuer_linked_domains(&issuer_did_document).await;
    for url in linked_domains {
        let validation_result = validate_domain_linkage(&resolver, url.clone(), aud).await;
        if validation_result.status == ValidationStatus::Success {
            public_verification_response.domain_linkage = ValidationResult {
                status: ValidationStatus::Success,
                payload: Some(url.to_string()),
            };
            break;
        }
    }

    // Validate the issuers linked verifiable presentations and then check if any of them were issued to this verifier to establish a trust relation.
    let linked_verifiable_credentials = validate_linked_verifiable_presentations(&resolver, &issuer_did_document)
        .await
        .into_iter()
        .flatten()
        .filter(|linked_verifiable_credential| {
            // Check if the issuer of the linked verifiable credential matches the DID of this verifier to establish a trust relation
            linked_verifiable_credential.issuer_linked_domains.iter().any(|domain| {
                let claims = match get_unverified_jwt_claims(&linked_verifiable_credential.data) {
                    Ok(claims) => claims,
                    Err(_) => return false,
                };
                // TODO: How to get the UniCores own DID('s)?

                // TODO: this is ugly but for now the easiest way for me to get all did_methods for the hackathon
                // In fact i don't need the full identifier (kid), just the DID.
                // for did_method in &enabled_did_methods {
                //     for alg in &supported_signing_algorithms {
                //         let did = state
                //             .subject
                //             .identifier(did_method.to_string().as_ref(), *alg)
                //             .await
                //             .map_err(|_| ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR).finish())?;

                //         dids.push(
                //             did.split('#')
                //                 .next()
                //                 .ok_or(ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR))?
                //                 .to_string(),
                //         );
                //     }
                // }
                claims
                    .get("aud")
                    .map(|aud| aud.as_str() == config().did_methods)
                    .unwrap_or(false)
            })
        })
        .collect();

    // TODO: validate status of Public Credential Token

    // Decode header to get kid
    let jwt_header = decode_header(&jwt).map_err(|_| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message("Failed to decode Public Credential Token header")
            .finish()
    })?;

    let kid = jwt_header.kid.ok_or_else(|| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message("Failed to get `kid` from Public Credential Token header")
            .finish()
    })?;

    // Fetch the public key using the kid
    let public_key = get_public_key_from_kid(&kid).await.map_err(|_| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message("Failed to retrieve public key for kid")
            .finish()
    })?;

    // Create decoding key based on the algorithm
    let decoding_key = match jwt_header.alg {
        Algorithm::EdDSA => DecodingKey::from_ed_der(&public_key),
        Algorithm::ES256 => DecodingKey::from_ec_der(&public_key),
        _ => {
            return Err(ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Token")
                .message(format!(
                    "Public Credential Token kid uses an unsupported algorithm: {:?}",
                    jwt_header.alg
                ))
                .finish());
        }
    };

    let validation = Validation::new(jwt_header.alg);

    // Decode and verify the JWT signature
    let _token_data = decode::<serde_json::Value>(&jwt, &decoding_key, &validation).map_err(|e| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message(format!("JWT verification failed: {}", e))
            .finish()
    })?;

    // All primary checks have passed for the Public Credential Token at this point, to perform the remaining checks we need to fetch the Public Credential from the Issuer.

    // Discover public credential endpoint through DID resolution
    let public_credential_endpoint = issuer_did_document
        .service()
        .iter()
        .find(|service| service.type_().contains("PublicCredential"))
        .and_then(|service| match service.service_endpoint() {
            ServiceEndpoint::One(url) => Some(url.clone()),
            // TODO: handle multiple endpoints?
            ServiceEndpoint::Set(urls) => urls.first().cloned(),
            ServiceEndpoint::Map(map) => map.values().next().and_then(|urls| urls.first().cloned()),
        })
        .ok_or_else(|| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Public Credential Endpoint Not Found")
                .message("Issuer DID Document is missing PublicCredentialEndpoint service")
                .finish()
        })?;

    // Fetch Public Credential from issuer endpoint
    let public_credential_endpoint_url_with_parameter =
        format!("{}?public-credential-token={}", public_credential_endpoint, jwt);
    let response = reqwest::get(public_credential_endpoint_url_with_parameter)
        .await
        .map_err(|e| {
            ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Failed to Fetch Public Credential")
                .message(format!(
                    "Failed to get response from Issuer Public Credential endpoint: {e}"
                ))
                .finish()
        })?;

    let verifiable_credential = response.json::<serde_json::Value>().await.map_err(|e| {
        ApiError::builder(StatusCode::BAD_GATEWAY)
            .title("Invalid Public Credential Response")
            .message(format!("Failed to parse Issuer Public Credential response: {e}"))
            .finish()
    })?;

    // Validate all remaining checks

    // Get unverified claims
    let verifiable_credential_claims = get_unverified_jwt_claims(&verifiable_credential).map_err(|_| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid JWT")
            .message("Failed to decode Public Credential")
            .finish()
    })?;

    // The subject of the Public Credential Token is the JTI (credential ID) of the issued credential which a Verifier is trying to access
    let sub = public_credential_token_claims
        .get("sub")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Token")
                .message("Failed to get `sub` claim from Public Credential Token")
                .finish()
        })?;

    let jti = verifiable_credential_claims
        .get("jti")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential")
                .message("Failed to get `jti` claim from Public Credential")
                .finish()
        })?;

    if sub != jti {
        return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Public Credential Token `sub` claim does not match Public Credential `jti` claim")
            .finish());
    }

    // TODO: none of this works with sd-jwt yet
    // Extract credential subject ID
    let credential_subject_id = verifiable_credential_claims.get("vc")
        .and_then(|data| data.get("credentialSubject"))
        .and_then(|cred_subject| cred_subject.get("id"))
        .and_then(|id| id.as_str())
        .ok_or_else(|| {
            ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Invalid Credential")
                .message("Requested credential is missing the credentialSubject.id field. Publicly sharing anonymous credentials is not supported.")
                .finish()
        })?;

    // Extract iss from claims and validate
    let iss = public_credential_token_claims
        .get("iss")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Token")
                .message("Failed to get `iss` claim from Public Credential Token")
                .finish()
        })?;

    // Check whether the issuer of the Public Credential Token matches the subject of the requested credential
    // This check means we currently don't allow to publicly share anonymous credentials
    if iss != credential_subject_id {
        return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Public Credential Token issuer does not match requested credential subject")
            .finish());
    }

    // TODO: validate status

    // Decode header to get kid
    let jwt_header = decode_header(&verifiable_credential.to_string()).map_err(|_| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message("Failed to decode Public Credential Token header")
            .finish()
    })?;

    let kid = jwt_header.kid.ok_or_else(|| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message("Failed to get `kid` from Public Credential Token header")
            .finish()
    })?;

    // Validate the kid belongs to the same DID as credential subject
    let kid_did = kid.split('#').next().unwrap_or(&kid);
    if kid_did != credential_subject_id {
        return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Public Credential Token kid does not match requested credential subject DID")
            .finish());
    }

    // Fetch the public key using the kid
    let public_key = get_public_key_from_kid(&kid).await.map_err(|_| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message("Failed to retrieve public key for kid")
            .finish()
    })?;

    // Create decoding key based on the algorithm
    let decoding_key = match jwt_header.alg {
        Algorithm::EdDSA => DecodingKey::from_ed_der(&public_key),
        Algorithm::ES256 => DecodingKey::from_ec_der(&public_key),
        _ => {
            return Err(ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Token")
                .message(format!(
                    "Public Credential Token kid uses an unsupported algorithm: {:?}",
                    jwt_header.alg
                ))
                .finish());
        }
    };

    let mut validation = Validation::new(jwt_header.alg);
    validation.set_issuer(&[credential_subject_id]);
    validation.set_audience(&[aud]);
    validation.sub = Some(sub.to_string());

    // Decode and verify the JWT signature
    let _token_data = decode::<serde_json::Value>(&jwt, &decoding_key, &validation).map_err(|e| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message(format!("JWT verification failed: {}", e))
            .finish()
    })?;

    // Return the credential if all validations pass
    Ok((StatusCode::OK, Json(public_verification_response)).into_response())
}

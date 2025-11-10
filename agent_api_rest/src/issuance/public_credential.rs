use crate::handlers::query_handler;
use agent_holder::credential::aggregate::get_unverified_jwt_claims;
use agent_issuance::state::IssuanceState;
use agent_secret_manager::subject::get_public_key_from_kid;
use agent_shared::config::{get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct PublicLinkQuery {
    public_credential_token: String,
}

/// This endpoint receives a Public Credential Token as a query parameter and then perform several validation steps on the token.
/// When all validations pass, the requested credential is returned in the response.
/// When any validation fails, only the error is returned.
/// Both the verifier and the Issuer need to perform all these checks on the Public Credential Token, zero trust is assumed.
pub async fn public_credential(
    State(state): State<IssuanceState>,
    Query(parameter): Query<PublicLinkQuery>,
) -> Result<Response, ApiError> {
    let jwt = parameter.public_credential_token;

    // Get unverified claims
    let jwt_value = serde_json::Value::String(jwt.clone());
    let claims = get_unverified_jwt_claims(&jwt_value).map_err(|_| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid JWT")
            .message("Failed to decode Public Credential Token")
            .finish()
    })?;

    // The subject of the Public Credential Token is the JTI (credential ID) of the issued credential which a Verifier is trying to access
    let sub = claims.get("sub").and_then(|v| v.as_str()).ok_or_else(|| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message("Failed to get `sub` claim from Public Credential Token")
            .finish()
    })?;

    // Get the credential, if there is one for the given `sub`
    let credential = query_handler(sub, &state.query.credential).await?.ok_or_else(|| {
        ApiError::builder(StatusCode::NOT_FOUND)
            .title("Invalid Token")
            .message("Public Credential Token `sub` claim does not correspond to a valid credential")
            .finish()
    })?;

    // Extract credential subject ID (we need as_ref to avoid moving credential)
    let credential_subject_id = credential
        .data
        .as_ref()
        .and_then(|d| d.raw.get("credentialSubject"))
        .and_then(|cs| cs.get("id"))
        .and_then(|id| id.as_str())
        .ok_or_else(|| {
            ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Invalid Credential")
                .message("Requested credential is missing the credentialSubject.id field. Publicly sharing anonymous credentials is not supported.")
                .finish()
        })?;

    // Extract iss from claims and validate
    let iss = claims.get("iss").and_then(|v| v.as_str()).ok_or_else(|| {
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

    // Extract the `aud` claim
    let aud = claims.get("aud").and_then(|v| v.as_str()).ok_or_else(|| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Token")
            .message("Failed to get `aud` claim from Public Credential Token")
            .finish()
    })?;

    // Validate that the `aud` matches an enabled DID of this Unicorn instance
    let supported_signing_algorithms = get_all_enabled_signing_algorithms_supported();
    let enabled_did_methods = get_all_enabled_did_methods();

    let mut dids = Vec::new();

    // TODO: this is ugly but for now the easiest way for me to get all did_methods for the hackathon
    // In fact i don't need the full identifier (kid), just the DID.
    for did_method in &enabled_did_methods {
        for alg in &supported_signing_algorithms {
            let did = state
                .subject
                .identifier(did_method.to_string().as_ref(), *alg)
                .await
                .map_err(|_| ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR).finish())?;

            dids.push(
                did.split('#')
                    .next()
                    .ok_or(ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR))?
                    .to_string(),
            );
        }
    }

    if !dids.contains(&aud.to_string()) {
        return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Public Credential Token audience does not match this holder's DID")
            .finish());
    }

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
    Ok((StatusCode::OK, Json(credential)).into_response())
}

#[cfg(test)]
pub mod tests {
    use crate::issuance::router;
    use crate::API_VERSION;
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use tower::Service as _;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_public_credential_endpoint_invalid_parameter() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state);

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!(
                        "{API_VERSION}/public-credential?public_credential_token=invalid"
                    ))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    }
}

use crate::handlers::query_handler;
use agent_holder::credential::aggregate::get_unverified_jwt_claims;
use agent_issuance::state::IssuanceState;
use agent_secret_manager::subject::Subject;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use oid4vc_core::authentication::verify::Verify;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct PublicLinkQuery {
    token_link: String,
}

pub async fn public_credential(
    State(state): State<IssuanceState>,
    Query(token): Query<PublicLinkQuery>,
) -> Result<Response, ApiError> {
    let jwt = token.token_link;
    let jwt_value = serde_json::Value::String(jwt.clone());

    // Get unverified claims
    let claims = get_unverified_jwt_claims(&jwt_value).map_err(|_| {
        ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Failed to decode public link token")
            .finish()
    })?;

    let sub = claims.get("sub").and_then(|v| v.as_str()).ok_or_else(|| {
        ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Missing sub in token")
            .finish()
    })?;

    // sub = jti = credential_id - fetch the credential
    let credential = query_handler(sub, &state.query.credential).await?.ok_or_else(|| {
        ApiError::builder(StatusCode::NOT_FOUND)
            .title("Invalid Token")
            .message("Token sub does not correspond to a valid credential")
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
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential")
                .message("Credential missing subject ID")
                .finish()
        })?;

    // Extract iss from claims and validate
    let iss = claims.get("iss").and_then(|v| v.as_str()).ok_or_else(|| {
        ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Missing iss in token")
            .finish()
    })?;

    if iss != credential_subject_id {
        return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Token issuer does not match credential subject")
            .finish());
    }

    // Decode header to get kid
    let jwt_header = decode_header(&jwt).map_err(|_| {
        ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Failed to decode token header")
            .finish()
    })?;

    let kid = jwt_header.kid.ok_or_else(|| {
        ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Missing kid in token header")
            .finish()
    })?;

    // Validate the kid belongs to the same DID as credential subject
    let kid_did = kid.split('#').next().unwrap_or(&kid);
    if kid_did != credential_subject_id {
        return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Token kid does not match credential subject DID")
            .finish());
    }

    let relying_party_state = Subject::default();

    // Fetch the public key using the kid
    let public_key = relying_party_state.public_key(&kid).await.map_err(|_| {
        ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message("Could not retrieve public key for kid")
            .finish()
    })?;

    // Create decoding key based on the algorithm
    let decoding_key = match jwt_header.alg {
        Algorithm::EdDSA => DecodingKey::from_ed_der(&public_key),
        Algorithm::ES256 => DecodingKey::from_ec_der(&public_key),
        _ => {
            return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
                .title("Invalid Token")
                .message(format!("Unsupported algorithm: {:?}", jwt_header.alg))
                .finish());
        }
    };

    let mut validation = Validation::new(jwt_header.alg);
    validation.set_issuer(&[credential_subject_id]);
    validation.sub = Some(sub.to_string());

    // Decode and verify the JWT signature
    let _token_data = decode::<serde_json::Value>(&jwt, &decoding_key, &validation).map_err(|e| {
        ApiError::builder(StatusCode::UNAUTHORIZED)
            .title("Invalid Token")
            .message(format!("JWT verification failed: {}", e))
            .finish()
    })?;

    // Return the credential if all validations pass
    Ok((StatusCode::OK, Json(credential)).into_response())
}

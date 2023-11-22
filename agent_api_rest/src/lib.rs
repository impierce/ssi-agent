use agent_issuance::{
    command::IssuanceCommand,
    handlers::{command_handler, query_handler},
    model::aggregate::IssuanceData,
    model::create_credential,
    queries::IssuanceDataView,
    state::DynApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Form, Router,
};
use axum_auth::AuthBearer;
use hyper::header;
use oid4vci::{credential_request::CredentialRequest, token_request::TokenRequest};
use serde_json::{json, Value};

// TODO: What to do with aggregate_id's?
const AGGREGATE_ID: &str = "agg-id-F39A0C";

pub fn app(state: DynApplicationState<IssuanceData, IssuanceDataView>) -> Router {
    Router::new()
        .route("/v1/credentials", post(create_unsigned_credential))
        .route(
            "/v1/openid4vci/.well-known/oauth-authorization-server",
            get(oauth_authorization_server),
        )
        .route(
            "/v1/openid4vci/.well-known/openid_credential_issuer",
            get(openid_credential_issuer),
        )
        .route("/v1/openid4vci/token", post(token))
        .route("/v1/openid4vci/credential", post(credential))
        .with_state(state)
}

#[axum_macros::debug_handler]
async fn create_unsigned_credential(
    State(state): State<DynApplicationState<IssuanceData, IssuanceDataView>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateUnsignedCredential {
        credential_subject: payload,
    };

    match create_credential(&state, command).await {
        Ok(_) => {}
        Err(err) => {
            println!("Error: {:#?}\n", err);
            return (StatusCode::BAD_REQUEST, err.to_string()).into_response();
        }
    };

    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => (
            StatusCode::CREATED,
            [(header::LOCATION, format!("/v1/credentials/{}", AGGREGATE_ID))],
            Json(view.subjects[0].credentials[0].unsigned_credential.clone()),
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[axum_macros::debug_handler]
async fn oauth_authorization_server(
    State(state): State<DynApplicationState<IssuanceData, IssuanceDataView>>,
) -> impl IntoResponse {
    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => (StatusCode::OK, Json(view.oid4vci_data.authorization_server_metadata)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[axum_macros::debug_handler]
async fn openid_credential_issuer(
    State(state): State<DynApplicationState<IssuanceData, IssuanceDataView>>,
) -> impl IntoResponse {
    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => (StatusCode::OK, Json(view.oid4vci_data.credential_issuer_metadata)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[axum_macros::debug_handler]
async fn token(
    State(state): State<DynApplicationState<IssuanceData, IssuanceDataView>>,
    Form(token_request): Form<TokenRequest>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateTokenResponse { token_request };

    match command_handler(AGGREGATE_ID.to_string(), &state, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    };

    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => (StatusCode::OK, Json(view.subjects[0].token_response.clone())).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[axum_macros::debug_handler]
async fn credential(
    State(state): State<DynApplicationState<IssuanceData, IssuanceDataView>>,
    AuthBearer(access_token): AuthBearer,
    Json(credential_request): Json<CredentialRequest>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateCredentialResponse {
        access_token,
        credential_request,
    };

    match command_handler(AGGREGATE_ID.to_string(), &state, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    };

    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => (StatusCode::OK, Json(view.subjects[0].credential_response.clone())).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_issuance::services::IssuanceServices;
    use agent_store::in_memory::InMemoryApplicationState;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[tokio::test]
    async fn location_header_is_set_on_successful_creation() {
        let state = Arc::new(InMemoryApplicationState::new(vec![], IssuanceServices {}).await)
            as DynApplicationState<IssuanceData, IssuanceDataView>;

        state
            .execute_with_metadata(
                "agg-id-F39A0C",
                IssuanceCommand::LoadCredentialFormatTemplate {
                    credential_format_template: serde_json::from_str(include_str!(
                        "../../agent_issuance/res/credential_format_templates/openbadges_v3.json"
                    ))
                    .unwrap(),
                },
                Default::default(),
            )
            .await
            .unwrap();

        state
            .execute_with_metadata(
                "agg-id-F39A0C",
                IssuanceCommand::CreateSubject {
                    pre_authorized_code: "test".to_string(),
                },
                Default::default(),
            )
            .await
            .unwrap();

        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/credentials")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credentialSubject": {
                                "first_name": "Ferris",
                                "last_name": "Rustacean",
                            },
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        assert_eq!(
            response.headers().get(http::header::LOCATION).unwrap(),
            "/v1/credentials/agg-id-F39A0C"
        );

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            serde_json::from_str::<Value>(
                r#"
                {
                    "@context": [
                        "https://www.w3.org/2018/credentials/v1",
                        "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json"
                    ],
                    "id": "http://example.com/credentials/3527",
                    "type": ["VerifiableCredential", "OpenBadgeCredential"],
                    "issuer": {
                        "id": "https://example.com/issuers/876543",
                        "type": "Profile",
                        "name": "Example Corp"
                    },
                    "issuanceDate": "2010-01-01T00:00:00Z",
                    "name": "Teamwork Badge",
                    "credentialSubject": {
                        "first_name": "Ferris",
                        "last_name": "Rustacean"
                    }
                }
                "#
            )
            .unwrap()
        );
    }
}

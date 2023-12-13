use agent_issuance::{
    handlers::query_handler, model::aggregate::IssuanceData, queries::IssuanceDataView, state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::AGGREGATE_ID;

#[axum_macros::debug_handler]
pub(crate) async fn openid_credential_issuer(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
) -> impl IntoResponse {
    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) if view.oid4vci_data.credential_issuer_metadata.is_some() => {
            (StatusCode::OK, Json(view.oid4vci_data.credential_issuer_metadata)).into_response()
        }
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{app, tests::BASE_URL};

    use super::*;
    use agent_issuance::{
        services::IssuanceServices,
        startup_commands::{create_credentials_supported, load_credential_issuer_metadata},
        state::{initialize, CQRS},
    };
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_oauth_authorization_server_endpoint() {
        let state = in_memory::ApplicationState::new(vec![], IssuanceServices {}).await;

        initialize(
            state.clone(),
            vec![
                load_credential_issuer_metadata(BASE_URL.clone()),
                create_credentials_supported(),
            ],
        )
        .await;

        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/.well-known/openid-credential-issuer")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            json!({
                "credential_issuer": "https://example.com/",
                "credential_endpoint": "https://example.com/openid4vci/credential",
                "credentials_supported": [{
                    "format": "jwt_vc_json",
                    "cryptographic_binding_methods_supported": [
                        "did:key"
                    ],
                    "cryptographic_suites_supported": [
                        "EdDSA"
                    ],
                    "credential_definition":{
                        "type": [
                            "VerifiableCredential",
                            "OpenBadgeCredential"
                        ]
                    },
                    "proof_types_supported": [
                        "jwt"
                    ]
                }]
            })
        );
    }
}

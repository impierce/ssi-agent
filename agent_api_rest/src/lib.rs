use agent_issuance::{
    command::IssuanceCommand, handlers::command_handler, model::aggregate::Credential, queries::CredentialView,
};
use agent_store::state::ApplicationState;
use axum::{
    extract::{Json, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use serde_json::{json, Value};

pub fn app(state: ApplicationState<Credential, CredentialView>) -> Router {
    Router::new()
        .route("/v1/credentials", post(create_credential_data))
        .with_state(state)
}

#[axum_macros::debug_handler]
async fn create_credential_data(
    State(state): State<ApplicationState<Credential, CredentialView>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateCredentialData { credential: payload };

    match command_handler("agg-id-F39A0C".to_string(), state, command).await {
        Ok(_) => (
            StatusCode::CREATED,
            [(header::LOCATION, format!("/v1/credentials/{}", "agg-id-F39A0C"))],
            Json(json!({
                "foo": "bar"
            })),
        )
            .into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, path::Path};

    use super::*;
    use agent_issuance::state::new_application_state;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use serde_json::json;
    use tower::ServiceExt;

    #[tokio::test]
    async fn location_header_is_set_on_successful_creation() {
        let state = new_application_state().await;
        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/credentials")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "first_name": "Ferris",
                            "last_name": "Rustacean",
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
            // serde_json::from_reader::<_, Value>(
            //     File::open(Path::new("../tests/response/create-open-badge.json")).unwrap()
            // )
            // .unwrap()
            serde_json::from_str::<Value>(
                r#"
                {
                    "@context": [
                        "https://www.w3.org/2018/credentials/v1",
                        "https://www.w3.org/2018/credentials/examples/v1",
                        "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json"
                    ],
                    "id": "http://example.edu/credentials/3732",
                    "type": ["VerifiableCredential", "OpenBadgeCredential"],
                    "issuer": {
                        "id": "https://example.edu/issuers/565049",
                        "type": ["IssuerProfile"],
                        "name": "Example University"
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

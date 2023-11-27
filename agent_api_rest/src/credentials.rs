use agent_issuance::{
    command::IssuanceCommand, handlers::query_handler, model::aggregate::IssuanceData,
    model::command_handler_without_id, queries::IssuanceDataView, state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use hyper::header;
use serde_json::Value;

use crate::AGGREGATE_ID;

#[axum_macros::debug_handler]
pub(crate) async fn credentials(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateUnsignedCredential {
        subject_id: payload["subjectId"].as_str().unwrap().parse().unwrap(),
        credential: payload["credential"].clone(),
    };

    match command_handler_without_id(&state, command).await {
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

#[cfg(test)]
mod tests {
    use crate::{
        app,
        tests::{create_subject, load_credential_format_template, SUBJECT_ID},
    };

    use super::*;
    use agent_issuance::services::IssuanceServices;
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_credentials_endpoint() {
        let state = Arc::new(in_memory::ApplicationState::new(vec![], IssuanceServices {}).await)
            as ApplicationState<IssuanceData, IssuanceDataView>;

        load_credential_format_template(state.clone()).await;
        create_subject(state.clone()).await;

        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/credentials")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "subjectId": SUBJECT_ID,
                            "credential": {"credentialSubject": {
                                "first_name": "Ferris",
                                "last_name": "Rustacean",
                            }},
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
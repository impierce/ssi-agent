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
pub(crate) async fn oauth_authorization_server(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
) -> impl IntoResponse {
    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) if view.oid4vci_data.authorization_server_metadata.is_some() => {
            (StatusCode::OK, Json(view.oid4vci_data.authorization_server_metadata)).into_response()
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
    use crate::{app, tests::load_authorization_server_metadata};

    use super::*;
    use agent_issuance::{services::IssuanceServices, state::CQRS};
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

        load_authorization_server_metadata(state.clone()).await;

        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/.well-known/oauth-authorization-server")
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
                "issuer": "https://example.com/",
                "token_endpoint": "https://example.com/auth/token"
            })
        );
    }
}

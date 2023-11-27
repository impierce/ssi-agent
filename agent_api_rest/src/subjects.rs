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
pub(crate) async fn subjects(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let pre_authorized_code = payload["value"].as_str().unwrap().to_string();
    let command = IssuanceCommand::CreateSubject {
        pre_authorized_code: pre_authorized_code.clone(),
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
            [(header::LOCATION, format!("/v1/subjects/{}", AGGREGATE_ID))],
            Json(
                view.subjects
                    .iter()
                    .find(|subject| subject.pre_authorized_code == pre_authorized_code)
                    .unwrap()
                    .clone(),
            ),
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
        tests::{create_subject, PRE_AUTHORIZED_CODE, SUBJECT_ID},
    };

    use super::*;
    use agent_issuance::{model::aggregate::IssuanceSubject, services::IssuanceServices};
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_subjects_endpoint() {
        let state = Arc::new(in_memory::ApplicationState::new(vec![], IssuanceServices {}).await)
            as ApplicationState<IssuanceData, IssuanceDataView>;

        create_subject(state.clone()).await;

        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/subjects")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "value": PRE_AUTHORIZED_CODE,
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
            "/v1/subjects/agg-id-F39A0C"
        );

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            serde_json::from_value::<IssuanceSubject>(body).unwrap(),
            IssuanceSubject {
                id: SUBJECT_ID.parse().unwrap(),
                pre_authorized_code: PRE_AUTHORIZED_CODE.to_string(),
                ..Default::default()
            }
        );
    }
}

use agent_issuance::{
    commands::IssuanceCommand, handlers::query_handler, model::aggregate::IssuanceData,
    model::command_handler_without_id, queries::IssuanceDataView, state::ApplicationState,
};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use hyper::header;
use serde_json::Value;

use crate::AGGREGATE_ID;

#[axum_macros::debug_handler]
pub(crate) async fn get_images(
    Path(id): Path<String>,
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
) -> impl IntoResponse {
    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => match view.images.get(&id) {
            Some(image) => image.data.clone().into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn post_images(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    // TODO: This should be removed once we know how to use aggregate ID's.
    let id = if let Some(id) = payload["id"].as_str() {
        id
    } else {
        return (StatusCode::BAD_REQUEST, "id is required".to_string()).into_response();
    };
    let data = if let Some(data) = payload["data"].as_str() {
        data
    } else {
        return (StatusCode::BAD_REQUEST, "data is required".to_string()).into_response();
    };

    let command = IssuanceCommand::UploadImage {
        id: id.to_string(),
        data: data.to_string(),
    };

    match command_handler_without_id(&state, command).await {
        Ok(_) => {}
        Err(err) => {
            println!("Error: {:#?}\n", err);
            return (StatusCode::BAD_REQUEST, err.to_string()).into_response();
        }
    };

    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => match view.images.get(id) {
            Some(data) => (
                StatusCode::CREATED,
                [(header::LOCATION, format!("/v1/images/{}", id))],
                Json(data),
            )
                .into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{app, tests::upload_image};

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
    async fn test_get_images_endpoint() {
        let state = Arc::new(in_memory::ApplicationState::new(vec![], IssuanceServices {}).await)
            as ApplicationState<IssuanceData, IssuanceDataView>;

        upload_image(state.clone()).await;

        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/v1/images/issuer-logo")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("Content-Type").unwrap().to_str().unwrap(),
            "text/plain; charset=utf-8"
        );

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();

        let image = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(image, "data:image/png;base64,iVBORw0KGgoAAAA");
    }

    #[tokio::test]
    async fn test_post_images_endpoint() {
        let state = Arc::new(in_memory::ApplicationState::new(vec![], IssuanceServices {}).await)
            as ApplicationState<IssuanceData, IssuanceDataView>;

        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/images")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "id": "issuer-logo",
                            "data": "data:image/png;base64,iVBORw0KGgoAAAA"
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
            "/v1/images/issuer-logo"
        );

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            json!({
                "id": "issuer-logo",
                "data": "data:image/png;base64,iVBORw0KGgoAAAA"
            })
        );
    }
}

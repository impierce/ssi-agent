use agent_holder::{presentation::aggregate::Presentation, state::HolderState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use std::sync::Arc;

use crate::{extractors::RequestActor, handlers::query_handler};

/// Get a signed credential presentation
///
/// Retrieves the compact JWT representation of a stored credential presentation.
#[utoipa::path(
    get,
    path = "/holder/presentations/{presentation_id}/signed",
    operation_id = "get_signed_holder_presentation",
    tags = ["Identity", "Holder"],
    params(
        ("presentation_id" = String, Path, description = "Credential presentation ID"),
    ),
    responses(
        (status = 200, description = "Signed credential presentation", body = String, content_type = "application/jwt"),
        (status = 400, description = "Invalid path parameter"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
        (status = 404, description = "Signed credential presentation not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn presentation_signed(
    State(state): State<Arc<HolderState>>,
    RequestActor(actor): RequestActor,
    Path(presentation_id): Path<String>,
) -> Result<Response, ApiError> {
    match query_handler(
        state.authorization_checker.clone(),
        actor,
        &presentation_id,
        Some(&presentation_id),
        &state.query.presentation,
    )
    .await?
    {
        Some(Presentation {
            signed: Some(signed_presentation),
            ..
        }) => Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/jwt")],
            signed_presentation.as_str().to_string(),
        )
            .into_response()),
        _ => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}

#[cfg(test)]
mod tests {
    use agent_holder::services::HolderServices;
    use agent_secret_manager::{service::Service as _, subject::Subject};
    use axum::{body::Body, extract::Request};
    use shared_kernel::authorization::{
        AuthorizationChecker, AuthorizationError, AuthorizationOperation, AuthorizationRequest, NoActorExtractor,
    };
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    #[derive(Default)]
    struct DenyingAuthorization {
        requests: Mutex<Vec<AuthorizationRequest>>,
    }

    #[async_trait::async_trait]
    impl AuthorizationChecker for DenyingAuthorization {
        async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            self.requests.lock().unwrap().push(request.clone());
            Err(AuthorizationError::Forbidden)
        }
    }

    #[tokio::test]
    async fn signed_presentation_is_subject_to_authorization() {
        let authorization = Arc::new(DenyingAuthorization::default());
        let mut state = agent_store::holder_state(
            &agent_store::in_memory::InMemory,
            Arc::new(HolderServices::new(Arc::new(Subject::test_subject().await))),
            &shared_kernel::EventBusHandle::default(),
        )
        .await;
        state.authorization_checker = authorization.clone();
        let app = crate::app_with_base_path(
            crate::ApiState {
                holder_state: Some(Arc::new(state)),
                ..Default::default()
            },
            Arc::new(NoActorExtractor),
            "/unicore/",
        );

        let response = app
            .oneshot(
                Request::get("/unicore/v0/holder/presentations/presentation-1/signed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 403);
        let requests = authorization.requests.lock().unwrap();
        assert!(matches!(
            &requests[..],
            [AuthorizationRequest {
                operation: AuthorizationOperation::Query {
                    resource_id: Some(resource_id),
                    ..
                },
                ..
            }] if resource_id == "presentation-1"
        ));
    }
}

use agent_issuance::{
    credential::command::CredentialCommand, offer::queries::access_token::AccessTokenView, state::IssuanceState,
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_auth::AuthBearer;
use oid4vci::{errors::NotificationError, notification_request::NotificationRequest};
//use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info};

/// The HTTP response MUST use the HTTP status code 400 (Bad Request) and set the content type to application/json.
/// Please Reference: https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0-13.html#name-notification-error-response

#[axum_macros::debug_handler]
pub async fn notification(
    State(state): State<IssuanceState>,
    AuthBearer(access_token): AuthBearer,
    Json(notification_request): Json<NotificationRequest>,
) -> Response {
    info!("Notification Request: {}", json!(notification_request));

    let _offer_id = match query_handler(&access_token, &state.query.access_token).await {
        Ok(Some(AccessTokenView { offer_id })) => offer_id,
        _ => {
            return NotificationError::InvalidToken.into_response();
        }
    };

    let credentials = match query_handler("all_credentials", &state.query.all_credentials).await {
        Ok(Some(all_credentials)) => all_credentials.credentials,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let credential_id = credentials
        .iter()
        .find(|entry| entry.1.notification_id.as_ref() == Some(&notification_request.notification_id))
        .map(|entry| entry.0.clone());

    let credential_id = match credential_id {
        Some(id) => id,
        None => {
            return NotificationError::InvalidNotificationId.into_response();
        }
    };

    let command = CredentialCommand::AddNotification {
        credential_id: credential_id.clone(),
        notification: notification_request,
    };

    if let Err(e) = command_handler(&credential_id, &state.command.credential, command).await {
        error!("Failed to add notification: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issuance::credential_issuer::credential::tests::credential;
    use crate::issuance::router;
    use crate::tests::BASE_URL;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_store::in_memory;
    use axum::{body::Body, http::Request};
    use oid4vci::notification_request::NotificationEvent;
    use serde_json;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_valid_notification_request() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;
        let mut app = router(issuance_state);

        let (access_token, notification_id) = credential(&mut app).await;

        let request = Request::builder()
            .uri("/openid4vci/notification")
            .method("POST")
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&NotificationRequest {
                    notification_id,
                    event: NotificationEvent::CredentialAccepted,
                    event_description: None,
                })
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}

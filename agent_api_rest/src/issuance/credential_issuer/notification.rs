use crate::handlers::{command_handler, query_handler};
use crate::issuance::error::{internal_server_error, PublicError};
use agent_issuance::{credential::command::CredentialCommand, state::IssuanceState};
use axum::response::{IntoResponse, Response};
use axum::{
    extract::{Json, State},
    http::StatusCode,
};
use axum_auth::AuthBearer;
use oid4vci::errors::NotificationErrorResponse;
use oid4vci::notification_request::NotificationRequest;
use serde_json::json;

use tracing::info;
/// The HTTP response MUST use the HTTP status code 400 (Bad Request) and set the content type to application/json.
/// Reference: https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0-13.html#name-notification-error-response

#[axum_macros::debug_handler]
pub async fn notification(
    State(state): State<IssuanceState>,
    AuthBearer(access_token): AuthBearer,
    Json(raw_value): Json<serde_json::Value>,
) -> Result<Response, PublicError> {
    info!("Notification Request: {}", json!(raw_value));

    let notification_request: NotificationRequest = serde_json::from_value::<NotificationRequest>(raw_value)
        .map_err(|_| PublicError::from(NotificationErrorResponse::InvalidNotificationRequest))?;

    let access_token_result = query_handler(&access_token, &state.query.access_token)
        .await
        .map_err(|_| internal_server_error())?;

    let _offer_id = match access_token_result {
        Some(access_token_view) => access_token_view.offer_id,
        None => {
            return Err(PublicError::from(NotificationErrorResponse::InvalidToken));
        }
    };

    let credentials = match query_handler("all_credentials", &state.query.all_credentials).await {
        Ok(Some(all_credentials)) => all_credentials.credentials,

        _ => return Err(internal_server_error()),
    };

    let credential_id = credentials
        .iter()
        .find(|entry| entry.1.notification_id.as_ref() == Some(&notification_request.notification_id))
        .map(|entry| entry.0.clone());

    let credential_id = match credential_id {
        Some(id) => id,
        None => {
            return Err(PublicError::from(NotificationErrorResponse::InvalidNotificationId));
        }
    };

    let command = CredentialCommand::AddNotification {
        credential_id: credential_id.clone(),
        notification: notification_request,
    };

    if command_handler(&credential_id, &state.command.credential, command)
        .await
        .is_err()
    {
        return Err(internal_server_error());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
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
    use oid4vci::errors::ErrorStatusCode;
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

    #[tokio::test]
    async fn test_invalid_notification_request() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;
        let mut app = router(issuance_state);

        let (access_token, notification_id) = credential(&mut app).await;

        struct TestCase {
            name: &'static str,
            access_token: String,
            payload: String,
            expected_error: NotificationErrorResponse,
        }

        let test_cases = vec![
            TestCase {
                name: "Invalid Notification ID",
                access_token: access_token.clone(),
                payload: serde_json::to_string(&NotificationRequest {
                    notification_id: "invalid_notification_id".to_string(),
                    event: NotificationEvent::CredentialAccepted,
                    event_description: None,
                })
                .unwrap(),
                expected_error: NotificationErrorResponse::InvalidNotificationId,
            },
            TestCase {
                name: "Invalid Access Token",
                access_token: "invalid_access_token".to_string(),
                payload: serde_json::to_string(&NotificationRequest {
                    notification_id: notification_id.clone(),
                    event: NotificationEvent::CredentialAccepted,
                    event_description: None,
                })
                .unwrap(),
                expected_error: NotificationErrorResponse::InvalidToken,
            },
            TestCase {
                name: "Invalid Notification Event",
                access_token: access_token.clone(),
                payload: format!(
                    r#"{{"notification_id": "{}", "event": "InvalidEventValue"}}"#,
                    notification_id
                ),
                expected_error: NotificationErrorResponse::InvalidNotificationRequest,
            },
        ];

        for test_case in test_cases {
            let request = Request::builder()
                .uri("/openid4vci/notification")
                .method("POST")
                .header("Authorization", format!("Bearer {}", test_case.access_token))
                .header("Content-Type", "application/json")
                .body(Body::from(test_case.payload))
                .unwrap();

            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                test_case.expected_error.status_code(),
                "Test case {} failed: expected status {}, got {}",
                test_case.name,
                test_case.expected_error.status_code(),
                response.status(),
            );
        }
    }
}

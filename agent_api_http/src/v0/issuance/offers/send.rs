use crate::extractors::RequestActor;
use crate::handlers::command_handler;
use agent_issuance::{offer::aggregate::DeliveryMethod, offer::command::OfferCommand, state::IssuanceState};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use url::Url;

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailOfferEndpointRequest {
    pub offer_id: String,
    pub recipient_email: String,
}

/// Send offer to individual
///
/// Sends a credential offer to an individual's email.
#[utoipa::path(
    post,
    path = "/offers/send-offer-to-individual",
    operation_id = "send_offer_to_individual",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Offer sent successfully"),
        (status = 400, description = "The credential offer does not exist"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn individual_offer(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Json(EmailOfferEndpointRequest {
        offer_id,
        recipient_email,
    }): Json<EmailOfferEndpointRequest>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        delivery_method: DeliveryMethod::Email { recipient_email },
    };

    // Send the Credential Offer to the recipient's email.
    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &offer_id,
        &state.command.offer,
        command,
    )
    .await?;

    Ok(StatusCode::OK.into_response())
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetUrlOfferEndpointRequest {
    pub offer_id: String,
    pub target_url: Url,
}

/// Send offer to organization
///
/// Sends a credential offer to an organization's URL.
#[utoipa::path(
    post,
    path = "/offers/send-offer-to-organization",
    operation_id = "send_offer_to_organization",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Offer sent successfully"),
        (status = 400, description = "The credential offer does not exist"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn organization_offer(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Json(TargetUrlOfferEndpointRequest { offer_id, target_url }): Json<TargetUrlOfferEndpointRequest>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        delivery_method: DeliveryMethod::TargetUrl { target_url },
    };

    // Send the offer to the organizational url.
    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &offer_id,
        &state.command.offer,
        command,
    )
    .await?;

    Ok(StatusCode::OK.into_response())
}

#[cfg(test)]
mod tests {
    use crate::tests::{OFFER_ID, TEMPLATE_ID};
    use crate::v0::issuance::{
        credentials::tests::{create_test_template, credentials, setup_library_state},
        offers::tests::offers,
        router,
    };
    use crate::API_VERSION;
    use agent_issuance::{issuance_state, services::IssuanceServices};
    use agent_secret_manager::service::Service as _;
    use agent_store::in_memory::InMemory;
    use axum::{
        body::{to_bytes, Body},
        extract::Request,
        http::StatusCode,
        Router,
    };
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tower::ServiceExt;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    /// Returns an issuance router holding the credential offer `OFFER_ID`.
    async fn app_with_offer() -> Router {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        agent_issuance::state::initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        create_test_template(&library_state).await;

        let mut app = router((issuance_state, library_state));
        credentials(&mut app).await;
        offers(&mut app, TEMPLATE_ID).await;

        app
    }

    async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    async fn post(app: &Router, path: &str, body: Value) -> StatusCode {
        send(
            app,
            Request::post(format!("{API_VERSION}{path}"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .0
    }

    async fn get(app: &Router, path: &str) -> (StatusCode, Value) {
        send(
            app,
            Request::get(format!("{API_VERSION}{path}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn an_offer_can_be_sent_to_an_organization() {
        let app = app_with_offer().await;
        let wallet = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/credential_offer"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&wallet)
            .await;

        let status = post(
            &app,
            "/offers/send-offer-to-organization",
            json!({ "offerId": OFFER_ID, "targetUrl": wallet.uri() }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let received_requests = wallet.received_requests().await.unwrap();
        assert!(received_requests[0]
            .url
            .query_pairs()
            .any(|(key, _)| key == "credential_offer" || key == "credential_offer_uri"));

        let (status, offer) = get(&app, &format!("/offers/{OFFER_ID}")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(offer["status"], "Pending");

        let (status, offers) = get(&app, "/offers").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(offers.as_array().unwrap().len(), 1);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn an_offer_rejected_by_the_organization_fails() {
        let app = app_with_offer().await;
        let wallet = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/credential_offer"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&wallet)
            .await;

        let status = post(
            &app,
            "/offers/send-offer-to-organization",
            json!({ "offerId": OFFER_ID, "targetUrl": wallet.uri() }),
        )
        .await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn an_offer_can_be_sent_to_an_individual() {
        let app = app_with_offer().await;

        let status = post(
            &app,
            "/offers/send-offer-to-individual",
            json!({ "offerId": OFFER_ID, "recipientEmail": "holder@example.com" }),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn unknown_offers_cannot_be_sent() {
        let app = app_with_offer().await;

        for (path, body) in [
            (
                "/offers/send-offer-to-individual",
                json!({ "offerId": "unknown", "recipientEmail": "holder@example.com" }),
            ),
            (
                "/offers/send-offer-to-organization",
                json!({ "offerId": "unknown", "targetUrl": "https://wallet.example.com" }),
            ),
        ] {
            assert_eq!(post(&app, path, body).await, StatusCode::BAD_REQUEST, "{path}");
        }

        let (status, _) = get(&app, "/offers/unknown").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

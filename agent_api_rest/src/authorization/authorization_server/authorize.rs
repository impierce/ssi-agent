use crate::issuance::error::PublicError;
use agent_authorization::application::oauth2_authorization_service::{
    OAuth2AuthorizationService, OAuth2AuthorizationServiceResponse,
};
use agent_authorization::state::AuthorizationState;
use axum::extract::Query;
use axum::response::Redirect;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use http::header;
use oid4vci::wallet::AuthorizationRequestByReference;

#[axum_macros::debug_handler]
pub(crate) async fn authorize(
    State(state): State<AuthorizationState>,
    Query(authorization_request): Query<AuthorizationRequestByReference>,
) -> Result<Response, PublicError> {
    match OAuth2AuthorizationService::handle_authorization_request(&state, authorization_request)
        .await
        .expect("FIXME")
    {
        OAuth2AuthorizationServiceResponse::RedirectToConsent(location) => Ok(Redirect::to(&location).into_response()),
        OAuth2AuthorizationServiceResponse::RedirectToClient(location) => {
            Ok((StatusCode::FOUND, [(header::LOCATION, location.to_string())]).into_response())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::authorization::authorization_server::consent::tests::{get_consent, post_consent};
    use crate::authorization::authorization_server::par::tests::par;
    use crate::issuance::credentials::tests::credentials;
    use crate::issuance::offers::tests::offers;
    use crate::{authorization, issuance};
    use agent_authorization::state::UNIME_CLIENT_ID;
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use agent_store::{authorization_state, issuance_state};
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::credential_offer::AuthorizationCode;
    use tower::Service as _;

    pub async fn authorize_before_consent(app: &mut Router, client_id: String, request_uri: String) -> String {
        let encoded_request_uri = urlencoding::encode(&request_uri);
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!(
                        "/auth/authorize?client_id={client_id}&request_uri={encoded_request_uri}",
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let see_other_location = response.headers().get("Location").unwrap().to_str().unwrap();

        assert_eq!(
            see_other_location,
            format!("/auth/consent?request_uri={encoded_request_uri}")
        );

        see_other_location.to_string()

        // get_consent(app, see_other_location.to_string()).await;

        // post_consent(app, client_id, request_uri.clone(), true).await
    }

    pub async fn authorize_after_consent(app: &mut Router, see_other_location: String) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(see_other_location)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FOUND);

        let found_location = response.headers().get("Location").unwrap().to_str().unwrap();
        assert!(found_location.starts_with("unime://callback?code="));

        let code = found_location.split("code=").nth(1).unwrap().to_string();

        code
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_authorization_endpoint() {
        let issuance_state = issuance_state::<InMemory>(Service::default(), Default::default()).await;

        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let mut app = issuance::router(issuance_state.clone());

        credentials(&mut app).await;
        let (authorization_code, _pre_authorized_code) = offers(&mut app, false).await.unwrap();
        let AuthorizationCode { issuer_state, .. } = authorization_code.unwrap();
        let issuer_state = issuer_state.unwrap();

        let authorization_state = authorization_state::<InMemory>(Service::default(), Default::default()).await;
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut app = authorization::router((authorization_state, issuance_state));

        let request_uri = par(&mut app, issuer_state).await;

        let see_other_location =
            authorize_before_consent(&mut app, UNIME_CLIENT_ID.to_string(), request_uri.clone()).await;

        get_consent(&mut app, see_other_location.clone()).await;

        let see_other_location = post_consent(&mut app, UNIME_CLIENT_ID.to_string(), request_uri, true).await;

        let _code = authorize_after_consent(&mut app, see_other_location).await;
    }
}

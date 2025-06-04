use crate::handlers::{command_handler, query_handler};
use crate::issuance::error::{internal_server_error, PublicError};
use agent_authorization::application::oauth2_authorization_service::{
    AuthorizationRequest, OAuth2AuthorizationService,
};
use agent_authorization::domain::oauth2_authorization_request::aggregate::OAuth2AuthorizationRequest;
use agent_authorization::state::AuthorizationState;
use axum::extract::Query;
use axum::response::Redirect;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use http::header;

lazy_static::lazy_static! {
    pub static ref TEMP_MAP: std::sync::Mutex<std::collections::HashMap<String, bool>> = {
        std::sync::Mutex::new(std::collections::HashMap::new())
    };
}

#[axum_macros::debug_handler]
pub(crate) async fn authorize(
    State(state): State<AuthorizationState>,
    Query(authorization_request): Query<AuthorizationRequest>,
) -> Result<Response, PublicError> {
    if TEMP_MAP
        .lock()
        .unwrap()
        .get(&authorization_request.request_uri.urn().to_string())
        != Some(&true)
    {
        return Ok(Redirect::to(&format!(
            "/auth/login?client_id={}&request_uri={}",
            authorization_request.client_id,
            authorization_request.request_uri.urn()
        ))
        .into_response());
    }

    let location = OAuth2AuthorizationService::handle_authorization_request(&state, authorization_request)
        .await
        .expect("FIXME");

    Ok((StatusCode::FOUND, [(header::LOCATION, location)]).into_response())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::authorization::authorization_server::login::LoginForm;
    use crate::authorization::authorization_server::par::tests::par;
    use crate::issuance::credentials::tests::credentials;
    use crate::issuance::offers::tests::offers;
    use crate::{authorization, issuance};
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use agent_store::{authorization_state, issuance_state};
    use axum::response::Html;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::credential_offer::AuthorizationCode;
    use tower::Service as _;

    pub async fn authorize(app: &mut Router, request_uri: String) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!(
                        "/auth/authorize?client_id=test_client_id&request_uri={request_uri}",
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
            "/auth/login?client_id=test_client_id&request_uri=urn:uuid:00000000-0000-0000-0000-000000000000"
        );

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

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "text/html; charset=utf-8"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("Login to Authorization Server"));
        assert!(html.contains("test_client_id"));
        assert!(html.contains("urn:uuid:00000000-0000-0000-0000-000000000000"));
        assert!(html.contains("action=\"/auth/login\""));
        assert!(html.contains("method=\"post\""));

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/login")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(
                        serde_urlencoded::to_string(&LoginForm {
                            username: "test_user".to_string(),
                            password: "test_password".to_string(),
                            client_id: "test_client_id".to_string(),
                            request_uri: "urn:uuid:00000000-0000-0000-0000-000000000000".to_string(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let see_other_location = response.headers().get("Location").unwrap().to_str().unwrap();
        assert_eq!(
            see_other_location,
            "/auth/authorize?client_id=test_client_id&request_uri=urn:uuid:00000000-0000-0000-0000-000000000000"
        );

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
        assert_eq!(
            found_location,
            "unime://callback?code=00000000-0000-0000-0000-000000000000"
        );

        let code = found_location.split("code=").nth(1).unwrap().to_string();

        code
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_authorization_endpoint() {
        let issuance_state = issuance_state::<InMemory>(Service::default(), Default::default()).await;

        initialize(&issuance_state).await.unwrap();

        let mut app = issuance::router(issuance_state.clone());

        credentials(&mut app).await;
        let (AuthorizationCode { issuer_state, .. }, _pre_authorized_code) = offers(&mut app).await.unwrap();
        let issuer_state = issuer_state.unwrap();

        let authorization_state = authorization_state::<InMemory>(Default::default()).await;
        let mut app = authorization::router((authorization_state, issuance_state));

        let request_uri = par(&mut app, issuer_state).await;

        let _code = authorize(&mut app, request_uri).await;
    }
}

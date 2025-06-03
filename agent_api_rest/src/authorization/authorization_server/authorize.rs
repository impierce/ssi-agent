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
        .get(&authorization_request.request_uri.to_string())
        != Some(&true)
    {
        return Ok(Redirect::to(&format!(
            "/auth/login?client_id={}&request_uri={}",
            authorization_request.client_id, authorization_request.request_uri
        ))
        .into_response());
    }

    let location = OAuth2AuthorizationService::handle_authorization_request(&state, authorization_request)
        .await
        .expect("FIXME");

    Ok((StatusCode::FOUND, [(header::LOCATION, "unime://callback?code=my-code")]).into_response())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::authorization::authorization_server::par::tests::par;
    use crate::issuance::credentials::tests::credentials;
    use crate::issuance::offers::tests::offers;
    use crate::{authorization, issuance};
    use agent_issuance::state::initialize;
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

    pub async fn authorize(app: &mut Router, request_uri: String) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!(
                        "/auth/authorize?client_id=test_client_id&request_uri={request_uri}",
                    ))
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(response.headers().get("Location").unwrap(), "unime://example?code=code");

        let code = response
            .headers()
            .get("Location")
            .unwrap()
            .to_str()
            .unwrap()
            .split("code=")
            .nth(1)
            .unwrap()
            .to_string();

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

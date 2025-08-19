use crate::authorization::authorization_server::templates::{ConsentPageTemplate, HtmlTemplate};
use crate::issuance::error::PublicError;
use crate::utils::StringifiedQuery;
use agent_authorization::application::consent_query_service::{ConsentPageViewModel, ConsentQueryService};
use agent_authorization::application::consent_service::{ConsentService, ConsentServiceResponse};
use agent_authorization::application::oauth2_authorization_service::GetLoginQuery;
use agent_authorization::state::AuthorizationState;
use axum::Form;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
};
use http::{header, StatusCode};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

#[axum_macros::debug_handler]
pub(crate) async fn get_consent(
    State(state): State<AuthorizationState>,
    StringifiedQuery(GetLoginQuery { request_uri }): StringifiedQuery<GetLoginQuery>,
) -> Result<Response, PublicError> {
    let ConsentPageViewModel {
        client_id,
        client_name,
        scope,
        authorization_details,
        request_uri,
    } = ConsentQueryService::prepare_consent_page_data(&state, request_uri)
        .await
        // TODO: implement proper error handling
        .map_err(|_err| PublicError::InternalServerError)?;

    Ok(HtmlTemplate(ConsentPageTemplate {
        client_name,
        client_id,
        scope,
        authorization_details,
        request_uri,
    })
    .into_response())
}

#[derive(Serialize, Deserialize)]
pub struct ConsentForm {
    pub client_id: String,
    pub request_uri: Uuid,
    pub consent_given: bool,
}

pub async fn post_consent(
    State(state): State<AuthorizationState>,
    Form(ConsentForm {
        client_id,
        request_uri,
        consent_given,
    }): Form<ConsentForm>,
) -> Result<Response, PublicError> {
    match ConsentService::handle_consent(&state, client_id, request_uri, consent_given)
        .await
        // TODO: implement proper error handling
        .map_err(|_err| PublicError::InternalServerError)?
    {
        ConsentServiceResponse::Found(location) => {
            info!("Redirecting to location: {}", location);

            Ok((StatusCode::FOUND, [(header::LOCATION, location)]).into_response())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::authorization::authorization_server::consent::ConsentForm;
    use agent_authorization::state::UNIME_CLIENT_ID;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use tower::Service as _;

    pub async fn get_consent(app: &mut Router, get_consent_location: String) {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(get_consent_location)
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
        assert!(html.contains("Credentials to be Shared"));
        assert!(html.contains(UNIME_CLIENT_ID));
        assert!(html.contains("action=\"/auth/consent\""));
        assert!(html.contains("method=\"post\""));
    }

    pub async fn post_consent(app: &mut Router, client_id: String, request_uri: String, consent_given: bool) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/consent")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(
                        serde_urlencoded::to_string(&ConsentForm {
                            client_id: client_id.clone(),
                            request_uri: request_uri.parse().unwrap(),
                            consent_given,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FOUND);

        let see_other_location = response.headers().get("Location").unwrap().to_str().unwrap();

        let encoded_request_uri = urlencoding::encode(&request_uri);

        assert_eq!(
            see_other_location,
            format!("/auth/authorize?client_id={client_id}&request_uri={encoded_request_uri}")
        );

        see_other_location.to_string()
    }
}

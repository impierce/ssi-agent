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
        .expect("FIXME");

    // FIXME
    Ok(HtmlTemplate(ConsentPageTemplate {
        title: "Consent to Client Access".to_string(),
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
        .expect("FIXME")
    {
        ConsentServiceResponse::Found(location) => {
            info!("Redirecting to location: {}", location);

            Ok((StatusCode::FOUND, [(header::LOCATION, location)]).into_response())
        }
    }
}

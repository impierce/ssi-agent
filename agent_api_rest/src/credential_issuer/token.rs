use agent_issuance::{
    command::IssuanceCommand,
    handlers::{command_handler, query_handler},
    model::aggregate::IssuanceData,
    queries::IssuanceDataView,
    state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    Form,
};
use oid4vci::token_request::TokenRequest;

use crate::AGGREGATE_ID;

#[axum_macros::debug_handler]
pub(crate) async fn token(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
    Form(token_request): Form<TokenRequest>,
) -> impl IntoResponse {
    let pre_authorized_code = match token_request.clone() {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code, ..
        } => pre_authorized_code,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let command = IssuanceCommand::CreateTokenResponse { token_request };

    match command_handler(AGGREGATE_ID.to_string(), &state, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    };

    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => {
            // TODO: This is a non-idiomatic way of finding the subject by using the pre-authorized_code in the token_request. We should use a aggregate/query instead.
            let subject = view
                .subjects
                .iter()
                .find(|subject| subject.pre_authorized_code == pre_authorized_code);
            if let Some(subject) = subject {
                (StatusCode::OK, Json(subject.token_response.clone())).into_response()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

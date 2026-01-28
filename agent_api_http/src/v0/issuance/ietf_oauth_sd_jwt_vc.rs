use crate::{handlers::query_handler, v0::issuance::error::PublicError};
use agent_issuance::state::{IssuanceState, SERVER_CONFIG_ID};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use identity_credential::sd_jwt_vc::metadata::TypeMetadata;
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn type_metadata(
    State(state): State<Arc<IssuanceState>>,
    Path((credential_configuration_id, _version)): Path<(String, String)>,
) -> Result<Response, PublicError> {
    let credential_configuration_id = URL_SAFE_NO_PAD
        .decode(credential_configuration_id)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or(PublicError::NotFoundError)?;

    // Check if the credential configuration IDs are valid.
    let _credential_configuration = query_handler(SERVER_CONFIG_ID, &state.query.server_config)
        .await?
        .and_then(|server_config_view| {
            server_config_view
                .credential_configurations
                .get(&credential_configuration_id)
                .map(|(_, credential_configuration, _authorization)| credential_configuration)
                .cloned()
        })
        .ok_or(PublicError::NotFoundError)?;

    // TODO: Fill in more of these fields once `agent_library` supports it.
    let type_metadata = TypeMetadata {
        name: Some(credential_configuration_id),
        description: None,
        extends: None,
        extends_integrity: None,
        schema: None,
        // TODO: Fill in display and claims once these issues are resolved: https://github.com/iotaledger/identity/pull/1770/changes#r2729345235
        display: vec![],
        claims: vec![],
    };

    Ok((axum::http::StatusCode::OK, axum::Json(type_metadata)).into_response())
}

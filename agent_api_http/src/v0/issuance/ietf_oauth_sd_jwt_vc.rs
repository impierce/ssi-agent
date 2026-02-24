use crate::{handlers::query_handler, v0::issuance::error::PublicError};
use agent_issuance::state::{IssuanceState, SERVER_CONFIG_ID};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use identity_credential::sd_jwt_vc::metadata::{
    ClaimDisclosability, ClaimDisplay, ClaimMetadata, DisplayMetadata, TypeMetadata,
};
use oid4vci::credential_issuer::credential_configurations_supported::{
    ClaimDescription, CredentialConfigurationsSupportedDisplay,
};
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
    let credential_configuration = query_handler(SERVER_CONFIG_ID, &state.query.server_config)
        .await?
        .and_then(|server_config_view| {
            server_config_view
                .credential_configurations
                .get(&credential_configuration_id)
                .map(|(_, credential_configuration, _authorization)| credential_configuration)
                .cloned()
        })
        .ok_or(PublicError::NotFoundError)?;

    let (display, claims) = credential_configuration
        .credential_metadata
        .map(|credential_metadata| {
            let display = credential_metadata
                .display
                .map(credential_configuration_display_to_display_metadata)
                .unwrap_or_default();
            let claims = credential_metadata
                .claims
                .map(claim_description_to_claims)
                .unwrap_or_default();

            (display, claims)
        })
        .unwrap_or_default();

    // TODO: Fill in more of these fields once `agent_library` supports it.
    // TODO: instead of contructing `TypeMetadata` here, we should store it as a View/Read Model and simply query it here.
    let type_metadata = TypeMetadata {
        name: Some(credential_configuration_id),
        description: None,
        extends: None,
        extends_integrity: None,
        schema: None,
        display,
        claims,
    };

    Ok((axum::http::StatusCode::OK, axum::Json(type_metadata)).into_response())
}

fn credential_configuration_display_to_display_metadata(
    supported_display: Vec<CredentialConfigurationsSupportedDisplay>,
) -> Vec<DisplayMetadata> {
    supported_display
        .into_iter()
        .map(|display| DisplayMetadata {
            locale: display.locale.unwrap_or_default(),
            name: display.name,
            description: display.description,
            rendering: None,
        })
        .collect()
}

fn claim_description_to_claims(claim_descriptions: Vec<ClaimDescription>) -> Vec<ClaimMetadata> {
    claim_descriptions
        .into_iter()
        .filter_map(|claim| {
            let display = claim
                .display
                .into_iter()
                .map(|display| ClaimDisplay {
                    locale: display.locale.unwrap_or_default(),
                    label: display.name,
                    description: None,
                })
                .collect();

            serde_json::from_value(serde_json::json!(claim.path))
                .ok()
                .map(|path| ClaimMetadata {
                    path,
                    display,
                    mandatory: None,
                    sd: Some(ClaimDisclosability::Always),
                    svg_id: None,
                })
        })
        .collect()
}

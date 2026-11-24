use crate::{
    handlers::public_query_handler,
    v0::{issuance::error::PublicError, openapi::PROTOCOL_TAG},
};
use agent_issuance::state::{IssuanceState, SERVER_CONFIG_ID};
use agent_shared::config::config;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use identity_kit::sd_jwt_vc::{ClaimDisplay, ClaimMetadata, ClaimPath, DisplayMetadata, SdPolicy, TypeMetadata};
use oid4vci::credential_format_profiles::CredentialFormats;
use oid4vci::credential_issuer::credential_configurations_supported::{
    ClaimDescription, CredentialConfigurationsSupportedDisplay,
};
use std::sync::Arc;

/// Get SD-JWT VC type metadata
///
/// Returns the type metadata of a credential configuration, as defined by
/// [SD-JWT VC](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-sd-jwt-vc#name-sd-jwt-vc-type-metadata).
#[utoipa::path(
    get,
    path = "/vct/{credential_configuration_id}/{version}",
    operation_id = "vct_type_metadata",
    tags = ["SD-JWT VC", PROTOCOL_TAG],
    params(
        (
            "credential_configuration_id" = String,
            Path,
            description = "Base64url-encoded (without padding) credential configuration ID",
        ),
        ("version" = String, Path, description = "Type version (currently ignored)"),
    ),
    responses(
        (status = 200, description = "SD-JWT VC type metadata", body = TypeMetadata),
        (status = 404, description = "The credential configuration does not exist"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn type_metadata(
    State(state): State<Arc<IssuanceState>>,
    Path((encoded_credential_configuration_id, version)): Path<(String, String)>,
) -> Result<Response, PublicError> {
    let credential_configuration_id = URL_SAFE_NO_PAD
        .decode(&encoded_credential_configuration_id)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or(PublicError::NotFoundError)?;

    // Check if the credential configuration IDs are valid.
    let credential_configuration = public_query_handler(SERVER_CONFIG_ID, &state.query.server_config)
        .await?
        .and_then(|server_config_view| {
            server_config_view
                .credential_configurations
                .get(&credential_configuration_id)
                .map(|(_, credential_configuration, _authorization)| credential_configuration)
                .cloned()
        })
        .ok_or(PublicError::NotFoundError)?;

    // Issuance conceals each top-level property of the credential subject (`vc+sd-jwt`) or of the credential
    // (`dc+sd-jwt`) as a whole.
    let top_level_path_len = match credential_configuration.credential_format {
        CredentialFormats::VcSdJwt(_) => 2,
        _ => 1,
    };
    let (display, claims) = credential_configuration
        .credential_metadata
        .map(|credential_metadata| {
            let display = credential_metadata
                .display
                .map(credential_configuration_display_to_display_metadata)
                .unwrap_or_default();
            let claims = credential_metadata
                .claims
                .map(|claims| claim_description_to_claims(claims, top_level_path_len))
                .unwrap_or_default();

            (display, claims)
        })
        .unwrap_or_default();

    // TODO: Fill in more of these fields once `agent_library` supports it.
    // TODO: instead of contructing `TypeMetadata` here, we should store it as a View/Read Model and simply query it here.
    let vct = format!(
        "{}vct/{encoded_credential_configuration_id}/{version}",
        config().public_url
    );
    let mut type_metadata = TypeMetadata::new(vct).with_name(credential_configuration_id);
    type_metadata.display = display;
    type_metadata.claims = claims;

    Ok((axum::http::StatusCode::OK, axum::Json(type_metadata)).into_response())
}

fn credential_configuration_display_to_display_metadata(
    supported_display: Vec<CredentialConfigurationsSupportedDisplay>,
) -> Vec<DisplayMetadata> {
    supported_display
        .into_iter()
        .map(|display| {
            let mut display_metadata = DisplayMetadata::new(display.locale.unwrap_or_default(), display.name);
            display_metadata.description = display.description;
            display_metadata
        })
        .collect()
}

/// Converts claims descriptions to the claims of type metadata. A top-level claim, whose path has
/// `top_level_path_len` elements, is always selectively disclosable; a claim inside one never is.
fn claim_description_to_claims(
    claim_descriptions: Vec<ClaimDescription>,
    top_level_path_len: usize,
) -> Vec<ClaimMetadata> {
    claim_descriptions
        .into_iter()
        .filter_map(|claim| {
            let sd = if claim.path.as_ref().len() == top_level_path_len {
                SdPolicy::Always
            } else {
                SdPolicy::Never
            };
            let path: ClaimPath = serde_json::from_value(serde_json::json!(claim.path)).ok()?;

            let mut claim_metadata = ClaimMetadata::new(path).with_sd(sd);
            claim_metadata.mandatory = claim.mandatory.then_some(true);
            claim_metadata.display = claim
                .display
                .into_iter()
                .filter_map(|display| Some(ClaimDisplay::new(display.locale.unwrap_or_default(), display.name?)))
                .collect();

            Some(claim_metadata)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v0::issuance::{
        credentials::tests::{create_test_template_with_status_and_format, setup_library_state},
        router,
    };
    use agent_issuance::{issuance_state, services::IssuanceServices};
    use agent_library::template::aggregate::Status;
    use agent_secret_manager::service::Service as _;
    use agent_store::in_memory::InMemory;
    use axum::{
        body::{to_bytes, Body},
        extract::Request,
        http::StatusCode,
        Router,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    /// Returns the issuance router and the ID of its only credential configuration, which uses the `vc+sd-jwt` format
    /// since only SD-JWT credential configurations describe their claims.
    async fn setup() -> (Router, String) {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        agent_issuance::state::initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        create_test_template_with_status_and_format(&library_state, Status::Published, None, "vc+sd-jwt").await;

        let credential_configuration_id = public_query_handler(SERVER_CONFIG_ID, &issuance_state.query.server_config)
            .await
            .unwrap()
            .unwrap()
            .credential_configurations
            .into_keys()
            .next()
            .unwrap();

        (router((issuance_state, library_state)), credential_configuration_id)
    }

    async fn get(app: &Router, encoded_credential_configuration_id: &str) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/vct/{encoded_credential_configuration_id}/1.0"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn type_metadata_is_derived_from_the_credential_configuration() {
        let (app, credential_configuration_id) = setup().await;

        let encoded_credential_configuration_id = URL_SAFE_NO_PAD.encode(&credential_configuration_id);
        let (status, type_metadata) = get(&app, &encoded_credential_configuration_id).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            type_metadata,
            serde_json::json!({
                "vct": format!("{}vct/{encoded_credential_configuration_id}/1.0", config().public_url),
                "name": credential_configuration_id,
                "display": [{ "locale": "", "name": "Verifiable Credential" }],
                // The template's schema requires `first_name` and `last_name`.
                "claims": [
                    { "path": ["credentialSubject", "id"], "sd": "always" },
                    { "path": ["credentialSubject", "first_name"], "mandatory": true, "sd": "always" },
                    { "path": ["credentialSubject", "last_name"], "mandatory": true, "sd": "always" },
                ]
            })
        );
    }

    #[test]
    fn mandatory_claims_are_marked_mandatory() {
        let claim_descriptions = serde_json::from_value(serde_json::json!([
            { "path": ["credentialSubject", "first_name"], "mandatory": true },
            { "path": ["credentialSubject", "last_name"] },
        ]))
        .unwrap();

        assert_eq!(
            serde_json::to_value(claim_description_to_claims(claim_descriptions, 2)).unwrap(),
            serde_json::json!([
                { "path": ["credentialSubject", "first_name"], "mandatory": true, "sd": "always" },
                { "path": ["credentialSubject", "last_name"], "sd": "always" },
            ])
        );
    }

    /// Only a top-level claim is selectively disclosable: a claim inside one is disclosed with it.
    #[rstest::rstest]
    #[case::vc_sd_jwt(serde_json::json!(["credentialSubject"]))]
    #[case::dc_sd_jwt(serde_json::json!([]))]
    fn only_top_level_claims_are_selectively_disclosable(#[case] prefix: serde_json::Value) {
        let path = |segments: serde_json::Value| {
            let mut path = prefix.as_array().unwrap().clone();
            path.extend(segments.as_array().unwrap().iter().cloned());
            path
        };
        let claim_descriptions = serde_json::from_value(serde_json::json!([
            { "path": path(serde_json::json!(["Address"])) },
            { "path": path(serde_json::json!(["Address", "Street"])) },
            { "path": path(serde_json::json!(["Courses"])) },
            { "path": path(serde_json::json!(["Courses", null, "Code"])) },
        ]))
        .unwrap();

        assert_eq!(
            serde_json::to_value(claim_description_to_claims(
                claim_descriptions,
                prefix.as_array().unwrap().len() + 1
            ))
            .unwrap(),
            serde_json::json!([
                { "path": path(serde_json::json!(["Address"])), "sd": "always" },
                { "path": path(serde_json::json!(["Address", "Street"])), "sd": "never" },
                { "path": path(serde_json::json!(["Courses"])), "sd": "always" },
                { "path": path(serde_json::json!(["Courses", null, "Code"])), "sd": "never" },
            ])
        );
    }

    #[tokio::test]
    async fn unknown_or_malformed_credential_configuration_ids_are_not_found() {
        let (app, _) = setup().await;

        for encoded_credential_configuration_id in [URL_SAFE_NO_PAD.encode("unknown"), "a".to_string()] {
            let (status, _) = get(&app, &encoded_credential_configuration_id).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{encoded_credential_configuration_id}");
        }
    }
}

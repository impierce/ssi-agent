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
use oid4vc_core::claim_path_pointer::ClaimPathPointer;
use oid4vci::credential_issuer::credential_configurations_supported::{
    ClaimDescription, CredentialConfigurationsSupportedDisplay,
};
use std::sync::Arc;

// OpenAPI representation of `identity_kit::sd_jwt_vc::TypeMetadata`.
/// Type metadata of an SD-JWT VC credential type.
///
/// See [SD-JWT VC Type Metadata](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-sd-jwt-vc#name-sd-jwt-vc-type-metadata).
#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
#[schema(as = TypeMetadata)]
pub(crate) struct TypeMetadataSchema {
    /// The type described, the URL this metadata is retrieved from.
    #[schema(format = Uri)]
    vct: String,
    /// A human-readable name for the type.
    name: Option<String>,
    /// A human-readable description for the type.
    description: Option<String>,
    /// A URI of another type that this type extends.
    #[schema(format = Uri)]
    extends: Option<String>,
    /// Integrity metadata for the extended type.
    #[serde(rename = "extends#integrity")]
    extends_integrity: Option<String>,
    #[serde(default)]
    display: Vec<TypeMetadataDisplaySchema>,
    #[serde(default)]
    claims: Vec<TypeMetadataClaimSchema>,
}

/// Display information for an SD-JWT VC type.
#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
#[schema(as = TypeMetadataDisplay)]
pub(crate) struct TypeMetadataDisplaySchema {
    /// A language tag as defined in [RFC 5646](https://www.rfc-editor.org/rfc/rfc5646.txt).
    locale: String,
    name: String,
    description: Option<String>,
    /// Rendering information, such as `simple` or `svg_templates`.
    rendering: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Information about particular claims of an SD-JWT VC type.
#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
#[schema(as = TypeMetadataClaim)]
pub(crate) struct TypeMetadataClaimSchema {
    path: ClaimPathPointer,
    #[serde(default)]
    display: Vec<TypeMetadataClaimDisplaySchema>,
    /// Whether the claim must be present in the issued credential.
    mandatory: Option<bool>,
    /// Whether the claim is selectively disclosable.
    #[schema(inline)]
    sd: Option<ClaimDisclosabilitySchema>,
    /// The ID of the claim for reference in an SVG template.
    svg_id: Option<String>,
}

/// Display information for a claim.
#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
#[schema(as = TypeMetadataClaimDisplay)]
pub(crate) struct TypeMetadataClaimDisplaySchema {
    /// A language tag as defined in [RFC 5646](https://www.rfc-editor.org/rfc/rfc5646.txt).
    locale: String,
    label: String,
    description: Option<String>,
}

#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ClaimDisclosabilitySchema {
    Always,
    Allowed,
    Never,
}

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
        (status = 200, description = "SD-JWT VC type metadata", body = TypeMetadataSchema),
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

fn claim_description_to_claims(claim_descriptions: Vec<ClaimDescription>) -> Vec<ClaimMetadata> {
    claim_descriptions
        .into_iter()
        .filter_map(|claim| {
            let path: ClaimPath = serde_json::from_value(serde_json::json!(claim.path)).ok()?;

            let mut claim_metadata = ClaimMetadata::new(path).with_sd(SdPolicy::Always);
            claim_metadata.mandatory = claim.mandatory.then_some(true);
            claim_metadata.display = claim
                .display
                .into_iter()
                .map(|display| ClaimDisplay::new(display.locale.unwrap_or_default(), display.name))
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
                "claims": [
                    { "path": ["credentialSubject", "id"], "sd": "always" },
                    { "path": ["credentialSubject", "first_name"], "sd": "always" },
                    { "path": ["credentialSubject", "last_name"], "sd": "always" },
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
            serde_json::to_value(claim_description_to_claims(claim_descriptions)).unwrap(),
            serde_json::json!([
                { "path": ["credentialSubject", "first_name"], "mandatory": true, "sd": "always" },
                { "path": ["credentialSubject", "last_name"], "sd": "always" },
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

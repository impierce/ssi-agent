pub mod send;

use crate::{
    error::type_url,
    handlers::{command_handler, query_handler},
};
use agent_issuance::{
    offer::{aggregate::DeliveryOptions, command::OfferCommand, views::OfferView},
    state::IssuanceState,
};
use agent_library::{
    state::LibraryState,
    template::aggregate::{Status as TemplateStatus, Template},
};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension,
};
use http_api_problem::ApiError;
use hyper::header;
use oid4vci::credential_offer::GrantType;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OffersEndpointRequest {
    pub offer_id: String,
    pub template_ids: Vec<String>,
    #[serde(default)]
    pub delivery_options: Option<DeliveryOptions>,
}

#[axum_macros::debug_handler]
pub(crate) async fn offers(
    State(state): State<Arc<IssuanceState>>,
    Extension(library_state): Extension<Arc<LibraryState>>,
    Json(OffersEndpointRequest {
        offer_id,
        template_ids,
        delivery_options,
    }): Json<OffersEndpointRequest>,
) -> Result<Response, ApiError> {
    if template_ids.is_empty() || template_ids.iter().any(|id| id.is_empty()) {
        return Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Missing Template IDs")
            .type_url(type_url("issuance#missing-template-ids"))
            .message("The `templateIds` field is required and all IDs must not be empty.")
            .finish());
    }

    // Validate and load all templates.
    let mut templates = Vec::with_capacity(template_ids.len());

    for template_id in &template_ids {
        let template: Template = query_handler(template_id, &library_state.query.template)
            .await?
            .filter(|t| t.status != TemplateStatus::Deleted)
            .ok_or_else(|| {
                ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                    .title("Template Not Found")
                    .type_url(type_url("issuance#template-not-found"))
                    .message(format!("No template found with id: `{template_id}`"))
                    .finish()
            })?;

        // Template must be in "Published" status
        if template.status != TemplateStatus::Published {
            return Err(ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Template Not Published")
                .type_url(type_url("issuance#template-not-published"))
                .message(format!(
                    "Template `{template_id}` must be Published to be used in an offer."
                ))
                .finish());
        }

        templates.push(template);
    }

    // Use first template for authorization/grant determination.
    let first_template = templates.first().expect("template_ids can not be empty");

    let authorization = first_template.holder_authorization.clone();

    let tx_code_constraints = authorization
        .pre_authorized
        .then_some(authorization.tx_code_constraints)
        .flatten();

    let grant_types = vec![if authorization.pre_authorized {
        GrantType::PreAuthorizedCode
    } else {
        GrantType::AuthorizationCode
    }];

    if query_handler(&offer_id, &state.query.offer).await?.is_none() {
        let command = OfferCommand::CreateCredentialOffer {
            offer_id: offer_id.clone(),
            template_ids,
            grant_types,
            tx_code_constraints,
            delivery_options,
        };

        command_handler(&offer_id, &state.command.offer, command).await?;
    }

    query_handler(&offer_id, &state.query.offer)
        .await?
        .and_then(|offer_view| offer_view.form_url_encoded_credential_offer)
        .map(|form_url_encoded_credential_offer| {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/x-www-form-urlencoded")],
                form_url_encoded_credential_offer,
            )
                .into_response()
        })
        // Unreachable error
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

/// Get all offers
///
/// List all credential offers.
#[utoipa::path(
    get,
    path = "/offers",
    operation_id = "get_all_offers",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "All offers retrieved successfully", body = [OfferView])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn all_offers(State(state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    let all_offers = query_handler("all_offers", &state.query.all_offers)
        .await?
        .map(|all_offers_view| all_offers_view.offers.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_offers)).into_response())
}

/// Get offer by ID
///
/// Retrieves a credential offer by its ID.
#[utoipa::path(
    get,
    path = "/offers/{offer_id}",
    operation_id = "get_offer_by_id",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Offer retrieved successfully", body = OfferView),
        (status = 404, description = "Offer not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn offer(
    State(state): State<Arc<IssuanceState>>,
    Path(offer_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&offer_id, &state.query.offer)
        .await?
        .map(|offer_view| (StatusCode::OK, Json(offer_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::tests::OFFER_ID;
    use crate::v0::issuance::{
        credentials::tests::{
            create_new_template, create_test_template, create_test_template_with_status_and_format,
            credentials_with_template, setup_library_state,
        },
        router,
    };
    use crate::API_VERSION;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::initialize;
    use agent_library::template::aggregate::{Expiration, Status};
    use agent_secret_manager::service::Service;
    use agent_shared::config::set_config;
    use agent_store::in_memory::InMemory;
    use agent_store::{issuance_state, library_state};
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
        Router,
    };
    use oid4vci::credential_offer::{
        AuthorizationCode, CredentialOffer, CredentialOfferParameters, Grants, PreAuthorizedCode,
    };
    use serde_json::{json, Value};
    use std::str::FromStr;
    use tower::Service as _;

    async fn post_offer_request(app: &mut Router, template_id: &str) -> Response {
        app.call(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("{API_VERSION}/offers"))
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "offerId": OFFER_ID,
                        "templateIds": vec![template_id],
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
    }

    pub async fn offers(
        app: &mut Router,
        template_id: &str,
    ) -> Option<(Option<AuthorizationCode>, Option<PreAuthorizedCode>)> {
        let response = post_offer_request(app, template_id).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "application/x-www-form-urlencoded"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: String = String::from_utf8(body.to_vec()).unwrap();

        match CredentialOffer::from_str(&body).unwrap() {
            CredentialOffer::CredentialOffer(credential_offer) => {
                assert_eq!(
                    &*credential_offer.credential_configuration_ids,
                    &vec![template_id.to_string()]
                );

                let CredentialOfferParameters {
                    grants:
                        Some(Grants {
                            authorization_code,
                            pre_authorized_code,
                        }),
                    ..
                } = *credential_offer
                else {
                    unreachable!()
                };

                Some((authorization_code, pre_authorized_code))
            }
            CredentialOffer::CredentialOfferUri(credential_offer_uri) => {
                assert_eq!(
                    credential_offer_uri,
                    url::Url::parse(&format!(
                        "https://my-domain.example.org/openid4vci/credential-offer/{OFFER_ID}"
                    ))
                    .unwrap()
                );

                None
            }
        }
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_offers_endpoint_requires_existing_template() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        let mut app = router((issuance_state, library_state));

        let response = post_offer_request(&mut app, "missing-template").await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["title"], "Template Not Found");
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_offers_endpoint_requires_template_id() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        let mut app = router((issuance_state, library_state));

        let response = post_offer_request(&mut app, "").await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["title"], "Missing Template IDs");
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_offers_endpoint_requires_published_template() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        let template_id = create_test_template_with_status_and_format(
            &library_state,
            Status::Draft,
            Some(Expiration::Never),
            "jwt_vc_json",
        )
        .await;

        let mut app = router((issuance_state, library_state));
        let response = post_offer_request(&mut app, &template_id).await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["title"], "Template Not Published");
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_offers_endpoint_accepts_published_template_without_pre_synced_configuration() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        let template_id = create_new_template(
            &library_state,
            Status::Published,
            Some(Expiration::Never),
            true,
            agent_library::template::aggregate::DataModel::W3CVcDataModelV1_1,
        )
        .await;

        let mut app = router((issuance_state, library_state));
        let response = post_offer_request(&mut app, &template_id).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "application/x-www-form-urlencoded"
        );
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_offers_endpoint() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;

        let mut app = router((issuance_state, library_state));

        credentials_with_template(&mut app, &template_id).await;
        let (_authorization_code, _pre_authorized_code) = offers(&mut app, &template_id).await.unwrap();
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_offers_endpoint_by_reference() {
        set_config().credential_offer_by_value_enabled = false;
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;

        let mut app = router((issuance_state, library_state));

        credentials_with_template(&mut app, &template_id).await;
        let none = offers(&mut app, &template_id).await;

        // When `credential_offer_by_value_enabled` is false, we expect no grants to be returned from the `offers` test function.
        assert!(none.is_none());

        set_config().credential_offer_by_value_enabled = true;
    }
}

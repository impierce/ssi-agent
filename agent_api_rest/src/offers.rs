use agent_issuance::{
    handlers::{command_handler, query_handler},
    offer::{aggregate::Offer, command::OfferCommand, queries::OfferView},
    state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::Value;

// use crate::AGGREGATE_ID;

// #[axum_macros::debug_handler]
pub(crate) async fn offers(
    State(state): State<ApplicationState<Offer, OfferView>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let subject_id = if let Some(subject_id) = payload["subjectId"].as_str() {
        subject_id
    } else {
        return (StatusCode::BAD_REQUEST, "subjectId is required".to_string()).into_response();
    };
    let pre_authorized_code = payload["preAuthorizedCode"].as_str().map(|s| s.to_string());
    let command = OfferCommand::CreateCredentialOffer {
        subject_id: subject_id.to_string(),
        pre_authorized_code,
    };

    match command_handler("OFFER-0123".to_string(), &state, command).await {
        Ok(_) => {}
        Err(err) => {
            println!("Error: {:#?}\n", err);
            return (StatusCode::BAD_REQUEST, err.to_string()).into_response();
        }
    };

    match query_handler("OFF-99988".to_string(), &state).await {
        // Ok(Some(view)) => {
        //     let credential_offer = view
        //         .subjects
        //         .iter()
        //         .find_map(|subject| {
        //             (subject.id == subject_id).then(|| {
        //                 subject
        //                     .credential_offer
        //                     .as_ref()
        //                     .map(|credential_offer| credential_offer.form_urlencoded.clone())
        //             })
        //         })
        //         .flatten();
        //     if let Some(credential_offer) = credential_offer {
        //         (StatusCode::OK, Json(credential_offer)).into_response()
        //     } else {
        //         StatusCode::NOT_FOUND.into_response()
        //     }
        //     .into_response()
        // }
        // Ok(None) => StatusCode::NOT_FOUND.into_response(),
        // Err(err) => {
        //     println!("Error: {:#?}\n", err);
        //     (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        // }
        _ => StatusCode::NOT_IMPLEMENTED.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        app,
        tests::{create_unsigned_credential, BASE_URL, PRE_AUTHORIZED_CODE, SUBJECT_ID},
    };

    use super::*;
    use agent_issuance::{
        services::IssuanceServices,
        startup_commands::{load_credential_format_template, load_credential_issuer_metadata},
        state::{initialize, CQRS},
    };
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use serde_json::json;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_offers_endpoint() {
        let state = in_memory::ApplicationState::new(vec![], IssuanceServices {}).await;

        initialize(
            state.clone(),
            vec![
                load_credential_format_template(),
                load_credential_issuer_metadata(BASE_URL.clone()),
            ],
        )
        .await;

        create_unsigned_credential(state.clone()).await;

        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/offers")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "subjectId": SUBJECT_ID,
                            "preAuthorizedCode": PRE_AUTHORIZED_CODE
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();

        let value: Value = serde_json::from_slice(&body).unwrap();
        let credential_offer = value.as_str().unwrap();
        assert_eq!(credential_offer, "openid-credential-offer://?credential_offer=%7B%22credential_issuer%22%3A%22https%3A%2F%2Fexample.com%2F%22%2C%22credentials%22%3A%5B%5D%2C%22grants%22%3A%7B%22urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Apre-authorized_code%22%3A%7B%22pre-authorized_code%22%3A%22pre-authorized_code%22%2C%22user_pin_required%22%3Afalse%7D%7D%7D");
    }
}

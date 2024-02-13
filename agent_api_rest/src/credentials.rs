use agent_issuance::{
    credential::command::CredentialCommand,
    handlers::{command_handler, query_handler},
    offer::command::OfferCommand,
    state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use hyper::header;
use serde_json::Value;

// use crate::AGGREGATE_ID;

#[axum_macros::debug_handler]
pub(crate) async fn credentials(
    State(state): State<ApplicationState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    // TODO: This should be removed once we know how to use aggregate ID's.
    let subject_id = if let Some(subject_id) = payload["subjectId"].as_str() {
        subject_id
    } else {
        return (StatusCode::BAD_REQUEST, "subjectId is required".to_string()).into_response();
    };

    // match command_handler(subject_id.to_string(), &state.offer, OfferCommand::CreateOffer).await {
    //     Ok(_) => {}
    //     Err(err) => {
    //         println!("{:?}", err)
    //     }
    // }

    let credential_id = uuid::Uuid::new_v4().to_string();

    match command_handler(
        credential_id.clone(),
        &state.credential,
        CredentialCommand::LoadCredentialFormatTemplate {
            credential_format_template: serde_json::from_str(include_str!(
                "../../agent_issuance/res/credential_format_templates/openbadges_v3.json"
            ))
            .unwrap(),
        },
    )
    .await
    {
        Ok(_) => {}
        Err(err) => {
            println!("{:?}", err)
        }
    }

    let command = CredentialCommand::CreateUnsignedCredential {
        // subject_id: subject_id.to_string(),
        // subject: Subject {
        //     pre_authorized_code: "MY_CODE_001".to_string(),
        // },
        credential: payload["credential"].clone(),
    };

    println!("command: {:#?}", command);

    match command_handler(credential_id.clone(), &state.credential, command).await {
        Ok(_) => {}
        Err(err) => {
            println!("{:?}", err)
        }
    }

    match command_handler(
        subject_id.to_string(),
        &state.offer,
        OfferCommand::AddCredential {
            credential_id: credential_id.clone(),
        },
    )
    .await
    {
        Ok(_) => {}
        Err(err) => {
            println!("{:?}", err)
        }
    }

    let pre_code = query_handler(subject_id.to_string(), &state.offer)
        .await
        .unwrap()
        .unwrap()
        .pre_authorized_code;

    dbg!(&pre_code);

    let test = state.offer.load_pre_authorized_code(&pre_code).await.unwrap();

    dbg!(test);

    match query_handler(credential_id.clone(), &state.credential).await {
        Ok(Some(view)) => {
            println!("view: {:?}", view);
            (
                StatusCode::CREATED,
                [(header::LOCATION, format!("/v1/credentials/{credential_id}"))],
                Json(view.credential.clone()),
            )
                .into_response()
            // match view.subjects.iter().find_map(|subject| {
            //     (subject.id == subject_id)
            //         .then(|| {
            //             subject
            //                 .credentials
            //                 .as_ref()
            //                 .map(|credential| credential.unsigned_credential.clone())
            //         })
            //         .flatten()
            // }) {
            //     Some(unsigned_credential) => (
            //         StatusCode::CREATED,
            //         [(header::LOCATION, format!("/v1/credentials/{}", "CRED-03"))],
            //         Json(unsigned_credential),
            //     )
            //         .into_response(),
            //     None => StatusCode::NOT_FOUND.into_response(),
            // }
        }
        Ok(None) => {
            println!("404");
            StatusCode::NOT_FOUND.into_response()
        }
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::convert::Infallible;

    use crate::{app, tests::SUBJECT_ID};

    use super::*;
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
        response::Response,
        Router,
    };
    use serde_json::json;
    use tower::Service;

    pub async fn credentials(app: &mut Router) -> Result<Response, Infallible> {
        app.call(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/credentials")
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "subjectId": SUBJECT_ID,
                        "credential": {
                            "credentialSubject": {
                            "first_name": "Ferris",
                            "last_name": "Rustacean"
                        }},
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
    }

    #[tokio::test]
    async fn test_credentials_endpoint() {
        let state = in_memory::application_state().await;

        // initialize(state.clone(), vec![load_credential_format_template()]).await;

        let mut app = app(state);

        let response = credentials(&mut app).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        // assert_eq!(
        //     response.headers().get(http::header::LOCATION).unwrap(),
        //     "/v1/credentials/agg-id-F39A0C"
        // );

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            json!({
                "@context": [
                    "https://www.w3.org/2018/credentials/v1",
                    "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json"
                ],
                "id": "http://example.com/credentials/3527",
                "type": ["VerifiableCredential", "OpenBadgeCredential"],
                "issuer": {
                    "id": "https://example.com/issuers/876543",
                    "type": "Profile",
                    "name": "Example Corp"
                },
                "issuanceDate": "2010-01-01T00:00:00Z",
                "name": "Teamwork Badge",
                "credentialSubject": {
                    "first_name": "Ferris",
                    "last_name": "Rustacean"
                }
            })
        );
    }
}

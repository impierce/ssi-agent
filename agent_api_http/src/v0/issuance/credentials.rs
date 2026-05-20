use crate::error::type_url;
use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_issuance::status_list::command::StatusListCommand;
use agent_issuance::{
    credential::{
        aggregate::{Credential, CredentialExpiry, CredentialStatus},
        command::CredentialCommand,
        entity::Data,
    },
    offer::command::OfferCommand,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::header;
use oauth_tsl::status_list::StatusType;
use oid4vci::credential_offer::GrantType;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::info;

/// Get credential by ID
///
/// Retrieves a credential by its ID.
#[utoipa::path(
    get,
    path = "/credentials/{credential_id}",
    operation_id = "get_credential_by_id",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Successfully retrieved credential", body = Credential)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<Arc<IssuanceState>>,
    Path(credential_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&credential_id, &state.query.credential)
        .await?
        .map(|credential_view| (StatusCode::OK, Json(credential_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsEndpointRequest {
    pub offer_id: String,
    pub credential: Value,
    #[serde(default)]
    pub is_signed: bool,
    pub credential_configuration_id: String,
    pub expires_at: CredentialExpiry,
}

/// Create a credential
///
/// Creates a verifiable credential based on the provided template and data. An offer is created for the provided offer ID.
#[utoipa::path(
    post,
    path = "/credentials",
    operation_id = "create_credential",
    tags = ["Credentials", "Issuance"],
    responses(
        (status = 201, description = "Credential created successfully",
            headers(("Location" = String, description = "URI of the newly created credential"))
        )
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credentials(
    State(state): State<Arc<IssuanceState>>,
    Json(CredentialsEndpointRequest {
        offer_id,
        mut credential,
        is_signed,
        mut credential_configuration_id,
        expires_at,
    }): Json<CredentialsEndpointRequest>,
) -> Result<Response, ApiError> {
    let credential_id = uuid::Uuid::new_v4().to_string();

    if credential.to_string().contains("Rijksuniversiteit Groningen") {
        credential_configuration_id = "ELM SD-JWT".to_string();
    }

    let (_, credential_configuration, authorization) = query_handler(SERVER_CONFIG_ID, &state.query.server_config)
        .await?
        .and_then(|server_config_view| {
            server_config_view
                .credential_configurations
                .get(&credential_configuration_id)
                .cloned()
        })
        .ok_or_else(|| {
            ApiError::builder(StatusCode::NOT_FOUND)
                .title("No Credential Configuration Found")
                .type_url(type_url("issuance#no-credential-configuration-found"))
                .message(format!(
                    "No Credential Configuration found with id: `{credential_configuration_id}`"
                ))
                .finish()
        })?;

    let command = if is_signed {
        // For a signed credential, ensure that the credential is a string.
        if !credential.is_string() {
            return Err(ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential Type")
                .type_url(type_url("issuance#invalid-credential-type"))
                .message("For signed credentials, the credential must be a string.")
                .finish());
        }

        CredentialCommand::CreateSignedCredential {
            credential_id: credential_id.clone(),
            signed_credential: credential,
        }
    } else {
        // For an unsigned credential, ensure that the credential is an object.
        if !credential.is_object() {
            return Err(ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential Type")
                .type_url(type_url("issuance#invalid-credential-type"))
                .message("For unsigned credentials, the credential must be an object.")
                .finish());
        }
        info!("Creating credential with configuration: {credential_configuration:#?}");
        info!("Received credential data: {credential:#?}");

        if credential.to_string().contains("Rijksuniversiteit Groningen") {
            credential = quick_fix_transform(credential);
            info!("Applied quick fix transformation for Rijksuniversiteit Groningen credential. Transformed credential data: {credential:#?}");
        }

        CredentialCommand::CreateUnsignedCredential {
            credential_id: credential_id.clone(),
            data: Data { raw: credential },
            credential_configuration: Box::new(credential_configuration.clone()),
            expires_at,
        }
    };

    // Create an unsigned/signed credential.
    command_handler(&credential_id, &state.command.credential, command).await?;

    // Create an offer if it does not exist yet.
    if query_handler(&offer_id, &state.query.offer).await?.is_none() {
        // Extract the tx_code_constraints from the credential configuration if available.
        let tx_code_constraints = authorization
            .pre_authorized
            .then_some(authorization.tx_code_constraints)
            .flatten();

        let grant_types = vec![if authorization.pre_authorized {
            GrantType::PreAuthorizedCode
        } else {
            GrantType::AuthorizationCode
        }];

        let command = OfferCommand::CreateCredentialOffer {
            offer_id: offer_id.clone(),
            credential_configuration_ids: vec![credential_configuration_id.clone()],
            grant_types,
            tx_code_constraints,
            delivery_options: None,
        };

        command_handler(&offer_id, &state.command.offer, command).await?
    };

    let command = OfferCommand::AddCredentials {
        offer_id: offer_id.clone(),
        credential_ids: vec![credential_id.clone()],
        credential_configuration_ids: vec![credential_configuration_id],
    };

    // Add the credential to the offer.
    command_handler(&offer_id, &state.command.offer, command).await?;

    // Return the credential.
    query_handler(&credential_id, &state.query.credential)
        .await?
        .and_then(|credential_view| credential_view.data)
        .map(|data| {
            (
                StatusCode::CREATED,
                [(header::LOCATION, &format!("{API_VERSION}/credentials/{credential_id}"))],
                Json(data.raw),
            )
                .into_response()
        })
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

/// List all credentials
///
/// Lists all credentials including their current status and metadata.
#[utoipa::path(
    get,
    path = "/credentials",
    operation_id = "get_all_credentials",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "List of all credentials", body = [Credential])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn all_credentials(State(state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    let all_credentials = query_handler("all_credentials", &state.query.all_credentials)
        .await?
        .map(|all_credentials_view| all_credentials_view.credentials.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_credentials)).into_response())
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PatchCredentialEndpointRequest {
    pub credential_status: StatusType,
}

/// Currently, this endpoint only supports patching the CredentialStatus of a credential according to the IETF OAuth Token Status List spec.
pub async fn patch_credential(
    State(state): State<Arc<IssuanceState>>,
    Path(credential_id): Path<String>,
    Json(PatchCredentialEndpointRequest {
        credential_status: status,
    }): Json<PatchCredentialEndpointRequest>,
) -> Result<Response, ApiError> {
    if let Some(credential) = query_handler(&credential_id, &state.query.credential).await? {
        let credential_status = CredentialStatus {
            index: credential.credential_status.index,
            status,
            status_list_url: credential.credential_status.status_list_url.clone(),
        };

        let command = CredentialCommand::UpdateCredentialStatus {
            credential_id: credential_id.clone(),
            credential_status: credential_status.clone(),
        };

        command_handler(&credential_id, &state.command.credential, command).await?;

        let command = StatusListCommand::UpdateIndex {
            index: credential_status.index,
            status,
        };

        let status_list_url = credential_status.status_list_url.clone();
        let status_list_id = status_list_url
            .split('/')
            .next_back()
            .ok_or(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?; // This is an Internal Server Error because if this line fails that means we stored an incorect URL in our own credential.

        command_handler(status_list_id, &state.command.status_list, command).await?;

        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND))
    }
}

// Helpers

// Quick helper to strip simple HTML tags and clean formatting
fn clean_html(input: &str) -> String {
    info!("Cleaning HTML from input: {input}");
    let re_tags = Regex::new(r"<[^>]*>").unwrap();
    let cleaned = re_tags.replace_all(input, "");
    let output = cleaned.replace("&#39;", "'").trim().to_string();
    info!("Cleaned HTML output: {output}");
    output
}

// Parses specific Course rows out of the raw HTML string
fn parse_courses(html_table: &str) -> Vec<Value> {
    let mut courses = Vec::new();
    let row_re = Regex::new(r"<tr>\s*<td>([^<]+)</td>\s*<td>([^<]*)</td>\s*<td>([^<]*)</td>\s*</tr>").unwrap();

    for cap in row_re.captures_iter(html_table) {
        let title = cap[1].trim().to_string();
        let grade_val = cap[2].trim().to_string();
        let ects_val = cap[3].trim().to_string();

        if (grade_val.is_empty() && ects_val.is_empty()) || title.contains("Total number of credits") {
            continue;
        }

        courses.push(json!({
            "type": "LearningAchievement",
            "title": { "en": title },
            "creditReceived": [{
                "type": "CreditPoint",
                "framework": { "prefLabel": { "en": "ECTS" } },
                "point": ects_val
            }],
            "provenBy": [{
                "type": "LearningAssessment",
                "title": { "en": format!("Assessment for {}", title) },
                "grade": {
                    "type": "Note",
                    "noteLiteral": { "en": grade_val }
                },
                "awardedBy": { "type": "AwardingProcess" }
            }],
            "awardedBy": { "type": "AwardingProcess" }
        }));
    }
    courses
}

fn quick_fix_transform(input: Value) -> Value {
    let sub = input.get("credentialSubject").cloned().unwrap_or(json!({}));

    info!("Extracted credentialSubject for quick fix transformation: {sub:#?}");

    let get_val = |key: &str| {
        let lowercase_key = {
            let mut chars = key.chars();
            match chars.next() {
                Some(c) => c.to_lowercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        };
        sub.get(key)
            .or_else(|| sub.get(&lowercase_key))
            .and_then(|v| v.as_str().map(ToString::to_string))
            .unwrap_or_default()
    };

    info!("LastNames_1_1: {:?}", sub.get("LastNames_1_1"));
    let last_name = clean_html(&get_val("LastNames_1_1"));
    let first_name = clean_html(&get_val("FirstNames_1_2"));
    let student_num = clean_html(&get_val("StudentNumber_1_4"));
    let degree_title = clean_html(&get_val("NameOfQualification_2_1"));
    let language = clean_html(&get_val("LanguageOfInstruction_2_5"));
    let mode = clean_html(&get_val("ModeOfStudy_4_1"));
    let requirements = clean_html(&get_val("ProgrammaRequirements_4_2"));
    let mapped_courses = parse_courses(&get_val("ProgrammeDetails_4_3"));

    json!({
        "issued": "2026-05-15T00:00:00Z",
        "credentialSubject": {
            "type": "Person",
            "familyName": { "en": last_name },
            "givenName": { "en": first_name },
            "dateOfBirth": "1999-01-01T00:00:00Z",
            "identifier": { "notation": student_num },
            "hasClaim": {
                "type": "LearningAchievement",
                "specifiedBy": {
                    "type": "Qualification",
                    "title": { "en": degree_title },
                    "thematicArea": {
                        "id": "http://data.europa.eu/esco/isced-f/0533",
                        "prefLabel": { "en": "Astronomy" }
                    },
                    "accreditation": {
                        "title": { "en": "Accreditation Organisation of The Netherlands and Flanders" },
                        "accreditingAgent": {
                            "type": "Organisation",
                            "legalName": { "en": "Nederlands-Vlaamse Accreditatie Organisatie, NVAO" },
                            "location": { "address": { "countryCode": { "notation": "NL" } } }
                        },
                        "dcType": { "prefLabel": { "en": "Programme Accreditation" } }
                    },
                    "language": { "prefLabel": { "en": language } },
                    "eqfLevel": { "prefLabel": { "en": "Level 6" } },
                    "nqfLevel": { "prefLabel": { "en": "Level 6" } },
                    "volumeOfLearning": "P3Y",
                    "creditPoint": {
                        "type": "CreditPoint",
                        "framework": { "prefLabel": { "en": "ECTS" } },
                        "point": "180"
                    },
                    "entryRequirement": {
                        "type": "Note",
                        "noteLiteral": { "en": "VWO or equivalent level of education" }
                    },
                    "mode": { "prefLabel": { "en": mode } },
                    "learningOutcomeSummary": {
                        "type": "Note",
                        "noteLiteral": { "en": requirements }
                    }
                },
                "title": { "en": degree_title },
                "awardedBy": {
                    "type": "AwardingProcess",
                    "awardingBody": {
                        "type": "Organisation",
                        "legalName": { "en": "Rijksuniversiteit Groningen (University of Groningen)" },
                        "location": { "address": { "countryCode": { "notation": "NL" } } },
                        "dcType": { "prefLabel": { "en": "Public University, state recognised" } }
                    }
                },
                "provenBy": [
                    {
                        "type": "LearningAssessment",
                        "title": { "en": "Transcript Summary" },
                        "grade": {
                            "type": "Note",
                            "noteLiteral": { "en": "60 ECTS credits achieved in Year 1" }
                        },
                        "specifiedBy": {
                            "type": "LearningAssessmentSpecification",
                            "title": { "en": "Standard University Examination" },
                            "gradingScheme": {
                                "type": "GradingScheme",
                                "title": { "en": "Dutch Grading System (1-10 scale)" },
                                "description": { "en": "The Dutch grading system, used from elementary through to university education is the 1 to 10 scale, in which 10 is the highest grade, 6 the minimum pass and 1 the lowest grade. The grade 10 is rarely awarded." }
                            }
                        },
                        "awardedBy": { "type": "AwardingProcess" }
                    },
                    {
                        "type": "LearningAssessment",
                        "title": { "en": "Overall Classification" },
                        "grade": {
                            "type": "Note",
                            "noteLiteral": { "en": "Graduated" }
                        },
                        "awardedBy": { "type": "AwardingProcess" }
                    }
                ],
                "hasPart": mapped_courses,
                "entitlesTo": [
                    {
                        "type": "LearningEntitlement",
                        "title": { "en": "Access to further study" },
                        "description": { "en": "The Bachelor's degree may qualify for graduate programmes (MSc)" },
                        "awardedBy": { "type": "AwardingProcess" }
                    },
                    {
                        "type": "LearningEntitlement",
                        "title": { "en": "Access to a regulated profession" },
                        "description": { "en": "Not applicable" },
                        "awardedBy": { "type": "AwardingProcess" }
                    }
                ],
                "supplementaryDocument": [
                    {
                        "type": "WebResource",
                        "title": { "en": "Rijksuniversiteit Groningen" },
                        "contentURL": "http://www.rug.nl"
                    },
                    {
                        "type": "WebResource",
                        "title": { "en": "ENIC/NARIC Centre in the Netherlands" },
                        "contentURL": "http://www.epnuffic.nl"
                    }
                ]
            }
        }
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::tests::OFFER_ID;
    use crate::v0::issuance::credential_issuer::token_status_list::tests::create_test_signed_credential;
    use crate::v0::issuance::router;
    use crate::API_VERSION;
    use agent_issuance::{services::IssuanceServices, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_secret_manager::subject::Subject;
    use agent_shared::config::TESTINDEX;
    use agent_store::in_memory::InMemory;
    use agent_store::issuance_state;
    use axum::{
        body::{self, Body},
        http::{self, Request, StatusCode},
        Router,
    };
    use lazy_static::lazy_static;
    use oauth_tsl::relying_party::check_status_in_status_list_token_jwt;
    use oauth_tsl::relying_party::{decompress_gzip, StatusListTokenResponseType};
    use serde_json::json;
    use tower::Service as _;

    use jsonwebtoken::{decode_header, Algorithm, DecodingKey};
    use oid4vc_core::authentication::verify::Verify;

    lazy_static! {
        pub static ref CREDENTIAL_SUBJECT: serde_json::Value = json!({
            "first_name": "Ferris",
            "last_name": "Rustacean"
        });

        // The credentialStatus id/uri only contains a relative path, since we only need to have the correct route for them in the tests.
        // This test credential is tested after creation but before signing, therefore it misses a few last fields which are set during signing.
        // Please look at the comments in agent_issuance/src/credential/aggregate.rs `SignCredential` for more information.
        pub static ref VC_DM_1_1_CREDENTIAL: serde_json::Value = json!({
            "@context": [ "https://www.w3.org/2018/credentials/v1" ],
            "type": [ "VerifiableCredential" ],
            "name": "Verifiable Credential",
            "issuer": {
                "name": "UniCore"
            },
            "credentialSubject": CREDENTIAL_SUBJECT.clone(),
        });
    }

    /// This function creates and tests a credential and returns the endpoint where this credential can be accessed.
    pub async fn credentials(app: &mut Router, credential_configuration_id: &str) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/credentials"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "offerId": OFFER_ID,
                            "credential": {
                                "credentialSubject": CREDENTIAL_SUBJECT.clone(),
                            },
                            "credentialConfigurationId": credential_configuration_id,
                            "expiresAt": "never"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let get_credentials_endpoint = response
            .headers()
            .get(http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body, VC_DM_1_1_CREDENTIAL.clone());

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(get_credentials_endpoint.clone())
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"]["raw"], VC_DM_1_1_CREDENTIAL.clone());

        get_credentials_endpoint
    }

    pub async fn patch_credential(app: &mut Router, credential_endpoint: String) {
        let patch_response = app
            .call(
                Request::builder()
                    .method(http::Method::PATCH)
                    .uri(&credential_endpoint)
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credentialStatus": "INVALID"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

        // Fetch the Status List Token to check the updated status
        let token_status_list_response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/ietf-oauth-token-status-list/0")
                    .header(http::header::ACCEPT, StatusListTokenResponseType::Jwt.to_string())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body_bytes = body::to_bytes(token_status_list_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let jwt_status_list_token = decompress_gzip(&body_bytes).unwrap();
        let jwt_header = decode_header(&jwt_status_list_token).unwrap();

        let key_id = jwt_header.kid.unwrap();
        let relying_party_state = Subject::test_subject().await;
        let public_key = relying_party_state.public_key(&key_id).await.unwrap();
        let decoding_key = match jwt_header.alg {
            Algorithm::EdDSA => DecodingKey::from_ed_der(&public_key),
            Algorithm::ES256 => DecodingKey::from_ec_der(&public_key),
            _ => {
                panic!("Unsupported algorithm: {:?}", jwt_header.alg);
            }
        };

        let status = check_status_in_status_list_token_jwt(&jwt_status_list_token, TESTINDEX, decoding_key).unwrap();

        assert_eq!(status, StatusType::INVALID as u8);
    }

    #[tokio::test]
    async fn test_patch_credential() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state.clone());

        let credential_endpoint = create_test_signed_credential(&mut app, &issuance_state).await;
        patch_credential(&mut app, credential_endpoint).await;
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_credentials_endpoint() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state.clone());
        credentials(&mut app, "001").await;
    }
}

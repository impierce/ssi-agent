use crate::error::{type_url, IntoApiErrorExt};
use crate::extractors::RequestActor;
use crate::handlers::{
    authorize_command, caller, command_handler, internal_command_handler, internal_query_handler, query_handler,
};
use crate::API_VERSION;
use agent_issuance::status_list::command::StatusListCommand;
use agent_issuance::{
    credential::{
        aggregate::{Credential, CredentialExpiry, CredentialStatus},
        command::CredentialCommand,
        entity::Data,
    },
    offer::{
        aggregate::{DeliveryMethod, DeliveryOptions},
        command::OfferCommand,
    },
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_library::queries;
use agent_library::state::LibraryState;
use agent_library::template::aggregate::{Expiration, Status as TemplateStatus, Template};
use agent_shared::config::Authorization;
use agent_shared::signed_credential_format::{detect_signed_credential_format, SignedCredentialFormat};
use axum::Extension;
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::header;
use oauth_tsl::status_list::StatusType;
use oid4vci::credential_offer::GrantType;
use oid4vci::{
    credential_format_profiles::CredentialFormats,
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shared_kernel::authorization::Actor;
use std::sync::Arc;

/// Get credential by ID
///
/// Retrieves a credential by its ID.
#[utoipa::path(
    get,
    path = "/credentials/{credential_id}",
    operation_id = "get_credential_by_id",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Successfully retrieved credential", body = Credential),
        (status = 400, description = "Invalid path parameter"),
        (status = 404, description = "Credential not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Path(credential_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &credential_id,
        Some(&credential_id),
        &state.query.credential,
    )
    .await?
    .map(|credential_view| (StatusCode::OK, Json(credential_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsEndpointRequest {
    pub template_id: String,
    pub offer_id: String,
    pub credential: Value,
    #[serde(default)]
    pub is_signed: bool,
    #[serde(default)]
    pub expires_at: Option<CredentialExpiry>,
}

/// One row of a batch: the claims for one holder.
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCredentialRow {
    /// The claims the credential asserts, validated as-is against the template schema. Values the
    /// schema fixes with `const` must be included; the server does not fill them in.
    pub claims: Value,
    /// When set, the offer is sent to this address once the credential is created.
    #[serde(default)]
    pub recipient_email: Option<String>,
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCredentialsRequest {
    /// The published template every row is validated against and issued from.
    pub template_id: String,
    /// Expiry applied to every credential; defaults to the template's credential expiration.
    #[serde(default)]
    pub expires_at: Option<CredentialExpiry>,
    /// One row per holder.
    pub credentials: Vec<BatchCredentialRow>,
}

/// One violation of the template schema by a credential's claims.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct SchemaViolation {
    /// JSON pointer to the offending value within the claims; empty for the claims object itself.
    pub path: String,
    pub message: String,
}

/// The RFC 9457 members of a problem, for embedding one problem inside another.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ProblemSummary {
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub type_url: Option<String>,
    pub detail: Option<String>,
    /// Present for `issuance#credential-schema-validation-failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub violations: Option<Vec<SchemaViolation>>,
}

impl From<&ApiError> for ProblemSummary {
    fn from(error: &ApiError) -> Self {
        let problem = error.to_http_api_problem();
        Self {
            title: problem.title,
            type_url: problem.type_url,
            detail: problem.detail,
            violations: error
                .fields()
                .get("violations")
                .cloned()
                .and_then(|violations| serde_json::from_value(violations).ok()),
        }
    }
}

/// The outcome of one row of a batch request.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchItemResult {
    /// 0-based index of the row in the request array.
    pub index: usize,
    /// `201` when the credential was created, `200` when the row was verified, otherwise the status
    /// of `error`.
    pub status: u16,
    /// Set once the credential exists, even if a later step of the row failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<String>,
    /// Set once the offer exists, even if a later step of the row failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer_id: Option<String>,
    /// The `openid-credential-offer://…` string a wallet consumes; what a QR code encodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_offer: Option<String>,
    /// Whether an email with the offer was requested for `recipientEmail`. Arrival is not tracked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_requested: Option<bool>,
    /// Why the row was rejected or could not be created; absent on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProblemSummary>,
}

impl BatchItemResult {
    fn verified(index: usize) -> Self {
        Self {
            index,
            status: StatusCode::OK.as_u16(),
            credential_id: None,
            offer_id: None,
            credential_offer: None,
            email_requested: None,
            error: None,
        }
    }

    fn created(index: usize, issued: Issued) -> Self {
        Self {
            index,
            status: StatusCode::CREATED.as_u16(),
            credential_id: issued.credential_id,
            offer_id: issued.offer_id,
            credential_offer: issued.credential_offer,
            email_requested: issued.email_requested,
            error: None,
        }
    }

    fn failed(index: usize, issued: Issued, error: &ApiError) -> Self {
        Self {
            index,
            status: error.status().as_u16(),
            credential_id: issued.credential_id,
            offer_id: issued.offer_id,
            credential_offer: issued.credential_offer,
            email_requested: issued.email_requested,
            error: Some(ProblemSummary::from(error)),
        }
    }
}

/// Problem details returned when no row of a batch request could be created.
#[derive(Serialize, utoipa::ToSchema)]
pub struct BatchFailedProblem {
    #[serde(rename = "type")]
    pub type_url: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    /// The outcome of every row, in request order.
    pub results: Vec<BatchItemResult>,
}

/// A published template together with the credential configuration derived from it.
struct ResolvedTemplate {
    template: Template,
    credential_configuration: CredentialConfigurationsSupportedObject,
    authorization: Authorization,
}

/// Loads the template and its credential configuration, rejecting templates that cannot issue.
async fn resolve_template(
    state: &Arc<IssuanceState>,
    actor: &Option<Actor>,
    library_state: &Arc<LibraryState>,
    template_id: &str,
) -> Result<ResolvedTemplate, ApiError> {
    if template_id.is_empty() {
        return Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Missing Template ID")
            .type_url(type_url("issuance#missing-template-id"))
            .message("The `templateId` field is required and must not be empty.")
            .finish());
    }

    let template: Template = queries::get_template(library_state, caller(actor.clone()), template_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .ok_or_else(|| {
            ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not Found")
                .type_url(type_url("issuance#template-not-found"))
                .message(format!("No template found with id: `{template_id}`"))
                .finish()
        })?;

    if template.status != TemplateStatus::Published {
        return Err(ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
            .title("Template Not Published")
            .type_url(type_url("issuance#template-not-published"))
            .message("Credential issuance requires the template to be Published.")
            .finish());
    }

    let (_, credential_configuration, authorization) = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        SERVER_CONFIG_ID,
        Some(SERVER_CONFIG_ID),
        &state.query.server_config,
    )
    .await?
    .and_then(|server_config_view| server_config_view.credential_configurations.get(template_id).cloned())
    .ok_or_else(|| {
        ApiError::builder(StatusCode::NOT_FOUND)
            .title("No Credential Configuration Found")
            .type_url(type_url("issuance#no-credential-configuration-found"))
            .message(format!("No Credential Configuration found with id: `{template_id}`"))
            .finish()
    })?;

    Ok(ResolvedTemplate {
        template,
        credential_configuration,
        authorization,
    })
}

impl ResolvedTemplate {
    /// Wraps claims the way the template's format expects: `dc+sd-jwt` carries them flat, the
    /// W3C-based formats under `credentialSubject`.
    fn credential_from_claims(&self, claims: Value) -> Value {
        if matches!(
            self.credential_configuration.credential_format,
            CredentialFormats::DcSdJwt(_)
        ) {
            claims
        } else {
            serde_json::json!({ "credentialSubject": claims })
        }
    }

    #[allow(clippy::result_large_err)]
    fn unsigned_credential_command(
        &self,
        credential_id: &str,
        credential: &Value,
        expires_at: &Option<CredentialExpiry>,
    ) -> Result<CredentialCommand, ApiError> {
        if !credential.is_object() {
            return Err(ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential Type")
                .type_url(type_url("issuance#invalid-credential-type"))
                .message("For unsigned credentials, the credential must be an object.")
                .finish());
        }

        if let Some(schema) = self.template.schema.as_ref() {
            let data_to_validate = credential.get("credentialSubject").unwrap_or(credential);
            validate_credential_against_schema(data_to_validate, schema).map_err(|e| *e)?;
        }

        let expires_at = match expires_at {
            Some(explicit) => {
                let template_deadline = expiration_to_credential_expiry(&self.template.credential_expiration)?;
                validate_expiry_within_template_deadline(explicit, &template_deadline)?;
                explicit.clone()
            }
            None => expiration_to_credential_expiry(&self.template.credential_expiration)?,
        };

        Ok(CredentialCommand::CreateUnsignedCredential {
            credential_id: credential_id.to_owned(),
            data: Data {
                raw: credential.clone(),
            },
            credential_configuration: Box::new(self.credential_configuration.clone()),
            expires_at,
        })
    }

    #[allow(clippy::result_large_err)]
    fn signed_credential_command(
        &self,
        credential_id: &str,
        credential: &Value,
    ) -> Result<CredentialCommand, ApiError> {
        let Some(signed_credential) = credential.as_str() else {
            return Err(ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential Type")
                .type_url(type_url("issuance#invalid-credential-type"))
                .message("For signed credentials, the credential must be a string.")
                .finish());
        };

        validate_signed_credential_format_matches_configuration(signed_credential, &self.credential_configuration)?;

        Ok(CredentialCommand::CreateSignedCredential {
            credential_id: credential_id.to_owned(),
            signed_credential: credential.clone(),
        })
    }

    fn create_offer_command(&self, offer_id: &str, delivery_options: Option<DeliveryOptions>) -> OfferCommand {
        // Extract the tx_code_constraints from the credential configuration if available.
        let tx_code_constraints = self
            .authorization
            .pre_authorized
            .then(|| self.authorization.tx_code_constraints.clone())
            .flatten();

        let grant_types = vec![if self.authorization.pre_authorized {
            GrantType::PreAuthorizedCode
        } else {
            GrantType::AuthorizationCode
        }];

        OfferCommand::CreateCredentialOffer {
            offer_id: offer_id.to_owned(),
            template_ids: vec![self.template.template_id.clone()],
            grant_types,
            tx_code_constraints,
            delivery_options,
        }
    }

    fn add_to_offer_command(&self, offer_id: &str, credential_id: &str) -> OfferCommand {
        OfferCommand::AddCredentials {
            offer_id: offer_id.to_owned(),
            credential_ids: vec![credential_id.to_owned()],
            template_ids: vec![self.template.template_id.clone()],
        }
    }
}

/// A validated credential and the commands that create and offer it.
struct ValidatedCredential {
    credential_id: String,
    offer_id: String,
    create_credential: CredentialCommand,
    create_offer: OfferCommand,
    add_to_offer: OfferCommand,
    send_offer: Option<OfferCommand>,
}

/// What executing a [`ValidatedCredential`] had produced when it finished or stopped.
#[derive(Default)]
struct Issued {
    credential_id: Option<String>,
    offer_id: Option<String>,
    credential_offer: Option<String>,
    email_requested: Option<bool>,
}

/// Creates the credential, creates its offer (or, if allowed, reuses an existing one), adds the
/// credential to the offer and, when requested, sends the offer.
///
/// On failure, returns what had been created up to that point alongside the error, so callers can
/// report it; events cannot be rolled back.
async fn execute(
    state: &Arc<IssuanceState>,
    actor: &Option<Actor>,
    item: ValidatedCredential,
    reuse_existing_offer: bool,
) -> Result<Issued, (ApiError, Issued)> {
    let ValidatedCredential {
        credential_id,
        offer_id,
        create_credential,
        create_offer,
        add_to_offer,
        send_offer,
    } = item;
    let mut issued = Issued::default();

    macro_rules! attempt {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Err((ApiError::from(error), issued)),
            }
        };
    }

    attempt!(
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &credential_id,
            &state.command.credential,
            create_credential,
        )
        .await
    );
    issued.credential_id = Some(credential_id.clone());

    let offer_exists = reuse_existing_offer
        && attempt!(
            query_handler(
                state.authorization_checker.clone(),
                actor.clone(),
                &offer_id,
                Some(&offer_id),
                &state.query.offer,
            )
            .await
        )
        .is_some();

    if !offer_exists {
        attempt!(
            command_handler(
                state.authorization_checker.clone(),
                actor.clone(),
                &offer_id,
                &state.command.offer,
                create_offer,
            )
            .await
        );
    }
    issued.offer_id = Some(offer_id.clone());

    attempt!(
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &offer_id,
            &state.command.offer,
            add_to_offer,
        )
        .await
    );

    issued.credential_offer = attempt!(
        internal_query_handler(
            state.authorization_checker.clone(),
            &offer_id,
            Some(&offer_id),
            &state.query.offer,
        )
        .await
    )
    .and_then(|offer_view| offer_view.form_url_encoded_credential_offer);

    issued.email_requested = Some(false);
    if let Some(send_offer) = send_offer {
        attempt!(
            command_handler(
                state.authorization_checker.clone(),
                actor.clone(),
                &offer_id,
                &state.command.offer,
                send_offer,
            )
            .await
        );
        issued.email_requested = Some(true);
    }

    Ok(issued)
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
        ),
        (status = 400, description = "Missing or empty `templateId`"),
        (status = 404, description = "Template not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credentials(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Extension(library_state): Extension<Arc<LibraryState>>,
    Json(request): Json<CredentialsEndpointRequest>,
) -> Result<Response, ApiError> {
    let is_signed = request.is_signed;

    let resolved = resolve_template(&state, &actor, &library_state, &request.template_id).await?;

    let credential_id = uuid::Uuid::new_v4().to_string();
    let create_credential = if is_signed {
        resolved.signed_credential_command(&credential_id, &request.credential)?
    } else {
        resolved.unsigned_credential_command(&credential_id, &request.credential, &request.expires_at)?
    };

    let item = ValidatedCredential {
        credential_id: credential_id.clone(),
        offer_id: request.offer_id.clone(),
        create_credential,
        create_offer: resolved.create_offer_command(&request.offer_id, None),
        add_to_offer: resolved.add_to_offer_command(&request.offer_id, &credential_id),
        send_offer: None,
    };

    execute(&state, &actor, item, true).await.map_err(|(error, _)| error)?;

    // Return the credential.
    internal_query_handler(
        state.authorization_checker.clone(),
        &credential_id,
        Some(&credential_id),
        &state.query.credential,
    )
    .await?
    .and_then(|credential_view| {
        if is_signed {
            credential_view.signed
        } else {
            credential_view.data.map(|data| data.raw)
        }
    })
    .map(|credential_body| {
        (
            StatusCode::CREATED,
            [(header::LOCATION, &format!("{API_VERSION}/credentials/{credential_id}"))],
            Json(credential_body),
        )
            .into_response()
    })
    .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

const MAX_BATCH_SIZE: usize = 1000;

/// Verify a batch of credentials
///
/// Checks every row of a batch as `create-credentials-batch` would — the template must be
/// published, each row's claims must satisfy its schema, and the caller must be allowed to create
/// the credentials and offers — without creating anything. Always answers `200` with the outcome of
/// every row; finding invalid rows is a successful verification. Values the template schema fixes
/// with `const` must be present in the claims; the server does not fill them in.
#[utoipa::path(
    post,
    path = "/verify-credentials-batch",
    operation_id = "verify_credentials_batch",
    tags = ["Credentials", "Issuance"],
    request_body = BatchCredentialsRequest,
    responses(
        (status = 200, description = "The outcome of every row, in request order", body = [BatchItemResult]),
        (status = 400, description = "Missing `templateId`, empty batch, or batch exceeds size limit"),
        (status = 404, description = "Template or its credential configuration not found"),
        (status = 422, description = "Template not published"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn verify_credentials_batch(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Extension(library_state): Extension<Arc<LibraryState>>,
    Json(request): Json<BatchCredentialsRequest>,
) -> Result<Response, ApiError> {
    let (validated, mut results) = validate_batch(&state, &actor, &library_state, &request).await?;

    authorize_execution(&state, &actor, validated.iter().map(|(_, item)| item)).await?;

    results.extend(validated.into_iter().map(|(index, _)| BatchItemResult::verified(index)));
    results.sort_by_key(|result| result.index);

    Ok((StatusCode::OK, Json(results)).into_response())
}

/// Create a batch of credentials
///
/// Creates one credential and one offer per row for a single template, in request order. Rows are
/// independent: a row that fails does not hold back the others, and a row that fails part-way
/// reports the IDs it did create. The response lists the outcome of every row: `201` when all were
/// created, `207` when some failed and `422` when none could be created. Rows with a
/// `recipientEmail` additionally have the offer sent to that address. Values the template schema
/// fixes with `const` must be present in the claims; the server does not fill them in.
#[utoipa::path(
    post,
    path = "/create-credentials-batch",
    operation_id = "create_credentials_batch",
    tags = ["Credentials", "Issuance"],
    request_body = BatchCredentialsRequest,
    responses(
        (status = 201, description = "All credentials created", body = [BatchItemResult]),
        (status = 207, description = "Some rows succeeded and some failed; see each row's `status`", body = [BatchItemResult]),
        (status = 400, description = "Missing `templateId`, empty batch, or batch exceeds size limit"),
        (status = 404, description = "Template or its credential configuration not found"),
        (status = 422, description = "Template not published, or no row could be created (`results` says why for each)",
            body = BatchFailedProblem, content_type = "application/problem+json"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn create_credentials_batch(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Extension(library_state): Extension<Arc<LibraryState>>,
    Json(request): Json<BatchCredentialsRequest>,
) -> Result<Response, ApiError> {
    let (validated, mut results) = validate_batch(&state, &actor, &library_state, &request).await?;

    authorize_execution(&state, &actor, validated.iter().map(|(_, item)| item)).await?;

    for (index, item) in validated {
        results.push(match execute(&state, &actor, item, false).await {
            Ok(issued) => BatchItemResult::created(index, issued),
            Err((error, issued)) => BatchItemResult::failed(index, issued, &error),
        });
    }
    results.sort_by_key(|result| result.index);

    let failed = results.iter().filter(|result| result.error.is_some()).count();
    if failed == results.len() {
        return Err(ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
            .title("Batch Failed")
            .type_url(type_url("issuance#batch-failed"))
            .message(format!("None of the {} credentials could be created.", results.len()))
            .field("results", results)
            .finish());
    }

    let status = if failed > 0 {
        StatusCode::MULTI_STATUS
    } else {
        StatusCode::CREATED
    };

    Ok((status, Json(results)).into_response())
}

/// Rejects malformed batches, resolves the template and validates every row. Rows that fail are
/// returned as results; rows that pass are returned with their index, ready to execute.
async fn validate_batch(
    state: &Arc<IssuanceState>,
    actor: &Option<Actor>,
    library_state: &Arc<LibraryState>,
    request: &BatchCredentialsRequest,
) -> Result<(Vec<(usize, ValidatedCredential)>, Vec<BatchItemResult>), ApiError> {
    if request.credentials.is_empty() {
        return Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Empty Batch")
            .type_url(type_url("issuance#empty-batch"))
            .message("The `credentials` array must contain at least one row.")
            .finish());
    }

    if request.credentials.len() > MAX_BATCH_SIZE {
        return Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Batch Too Large")
            .type_url(type_url("issuance#batch-too-large"))
            .message(format!(
                "Batch size {} exceeds the maximum of {MAX_BATCH_SIZE}.",
                request.credentials.len()
            ))
            .finish());
    }

    let resolved = resolve_template(state, actor, library_state, &request.template_id).await?;

    let mut validated = Vec::with_capacity(request.credentials.len());
    let mut results = Vec::new();
    for (index, row) in request.credentials.iter().enumerate() {
        match validate_row(&resolved, row, &request.expires_at) {
            Ok(item) => validated.push((index, item)),
            Err(error) => results.push(BatchItemResult::failed(index, Issued::default(), &error)),
        }
    }

    Ok((validated, results))
}

#[allow(clippy::result_large_err)]
fn validate_row(
    resolved: &ResolvedTemplate,
    row: &BatchCredentialRow,
    expires_at: &Option<CredentialExpiry>,
) -> Result<ValidatedCredential, ApiError> {
    if !row.claims.is_object() {
        return Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid Credential Type")
            .type_url(type_url("issuance#invalid-credential-type"))
            .message("`claims` must be an object.")
            .finish());
    }

    let credential_id = uuid::Uuid::new_v4().to_string();
    let offer_id = uuid::Uuid::new_v4().to_string();

    let credential = resolved.credential_from_claims(row.claims.clone());
    let create_credential = resolved.unsigned_credential_command(&credential_id, &credential, expires_at)?;

    // A blank address means "no email", so a CSV with an empty cell does not fail the row.
    let recipient_email = row
        .recipient_email
        .as_deref()
        .map(str::trim)
        .filter(|recipient_email| !recipient_email.is_empty())
        .map(str::to_owned);

    let delivery_options = recipient_email.clone().map(|recipient_email| DeliveryOptions {
        recipient_email: Some(recipient_email),
    });
    let send_offer = recipient_email.map(|recipient_email| OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        delivery_method: DeliveryMethod::Email { recipient_email },
    });

    Ok(ValidatedCredential {
        create_offer: resolved.create_offer_command(&offer_id, delivery_options),
        add_to_offer: resolved.add_to_offer_command(&offer_id, &credential_id),
        credential_id,
        offer_id,
        create_credential,
        send_offer,
    })
}

/// Checks that the caller may execute every command the rows would issue, so that a verify run
/// reports authorization failures and a create run does not fail row by row for them.
async fn authorize_execution(
    state: &Arc<IssuanceState>,
    actor: &Option<Actor>,
    items: impl Iterator<Item = &ValidatedCredential>,
) -> Result<(), ApiError> {
    let authorization_checker = state.authorization_checker.as_ref();

    for item in items {
        authorize_command(
            authorization_checker,
            actor.clone(),
            &item.credential_id,
            &item.create_credential,
        )
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

        let offer_commands = [&item.create_offer, &item.add_to_offer]
            .into_iter()
            .chain(item.send_offer.as_ref());
        for command in offer_commands {
            authorize_command(authorization_checker, actor.clone(), &item.offer_id, command)
                .await
                .map_err(IntoApiErrorExt::into_api_error)?;
        }
    }

    Ok(())
}

#[allow(clippy::result_large_err)]
fn validate_signed_credential_format_matches_configuration(
    signed_credential: &str,
    credential_configuration: &CredentialConfigurationsSupportedObject,
) -> Result<(), ApiError> {
    let actual_format = detect_signed_credential_format(signed_credential)
        .map_err(|e| invalid_signed_credential_format_error(e.to_string()))?;
    let expected_format = expected_signed_credential_format(credential_configuration)?;

    if actual_format == expected_format {
        Ok(())
    } else {
        Err(ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
            .title("Signed Credential Format Mismatch")
            .type_url(type_url("issuance#signed-credential-format-mismatch"))
            .message(format!(
                "Signed credential format `{}` does not match template credential configuration format `{}`.",
                actual_format.as_str(),
                expected_format.as_str()
            ))
            .finish())
    }
}

#[allow(clippy::result_large_err)]
fn expected_signed_credential_format(
    credential_configuration: &CredentialConfigurationsSupportedObject,
) -> Result<SignedCredentialFormat, ApiError> {
    match &credential_configuration.credential_format {
        CredentialFormats::JwtVcJson(_) => Ok(SignedCredentialFormat::JwtVcJson),
        CredentialFormats::VcSdJwt(_) => Ok(SignedCredentialFormat::VcSdJwt),
        CredentialFormats::DcSdJwt(_) => Ok(SignedCredentialFormat::DcSdJwt),
        _ => Err(ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
            .title("Unsupported Credential Configuration Format")
            .type_url(type_url("issuance#unsupported-credential-configuration-format"))
            .message("The template-backed credential configuration uses an unsupported credential format.")
            .finish()),
    }
}

fn invalid_signed_credential_format_error(message: impl Into<String>) -> ApiError {
    ApiError::builder(StatusCode::BAD_REQUEST)
        .title("Invalid Signed Credential Format")
        .type_url(type_url("issuance#invalid-signed-credential-format"))
        .message(message.into())
        .finish()
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
pub(crate) async fn all_credentials(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let all_credentials = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_credentials",
        None,
        &state.query.all_credentials,
    )
    .await?
    .map(|all_credentials_view| crate::utils::newest_first(all_credentials_view.credentials).collect::<Vec<_>>())
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_credentials)).into_response())
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchCredentialEndpointRequest {
    #[schema(schema_with = agent_issuance::credential::openapi::status_type)]
    pub credential_status: StatusType,
}

/// Update credential status
///
/// Updates a credential's status according to the IETF OAuth Token Status List specification.
#[utoipa::path(
    patch,
    path = "/credentials/{credential_id}",
    operation_id = "update_credential_status",
    tags = ["Issuance"],
    request_body(
        content = PatchCredentialEndpointRequest,
        example = json!({ "credentialStatus": "INVALID" })
    ),
    responses(
        (status = 204, description = "Credential status updated successfully"),
        (status = 400, description = "Malformed JSON request body"),
        (status = 404, description = "Credential not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
pub async fn patch_credential(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Path(credential_id): Path<String>,
    Json(PatchCredentialEndpointRequest {
        credential_status: status,
    }): Json<PatchCredentialEndpointRequest>,
) -> Result<Response, ApiError> {
    if let Some(credential) = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &credential_id,
        Some(&credential_id),
        &state.query.credential,
    )
    .await?
    {
        let credential_status = CredentialStatus {
            index: credential.credential_status.index,
            status,
            status_list_url: credential.credential_status.status_list_url.clone(),
        };

        let command = CredentialCommand::UpdateCredentialStatus {
            credential_id: credential_id.clone(),
            credential_status: credential_status.clone(),
        };

        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &credential_id,
            &state.command.credential,
            command,
        )
        .await?;

        let command = StatusListCommand::UpdateIndex {
            index: credential_status.index,
            status,
        };

        let status_list_url = credential_status.status_list_url.clone();
        let status_list_id = status_list_url
            .split('/')
            .next_back()
            .ok_or(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?; // This is an Internal Server Error because if this line fails that means we stored an incorect URL in our own credential.

        internal_command_handler(
            state.authorization_checker.clone(),
            status_list_id,
            &state.command.status_list,
            command,
        )
        .await?;

        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND))
    }
}

/// Converts a template's `Expiration` value into a `CredentialExpiry` for the issuance pipeline.
///
/// - `Never` → `CredentialExpiry::Never`
/// - `DateTime(s)` → `CredentialExpiry::Fixed(parsed datetime)`
/// - `Duration(s)` → `CredentialExpiry::Fixed(Utc::now() + duration)`
#[allow(clippy::result_large_err)]
fn expiration_to_credential_expiry(expiration: &Expiration) -> Result<CredentialExpiry, ApiError> {
    match expiration {
        Expiration::Never => Ok(CredentialExpiry::Never),
        Expiration::DateTime(s) => chrono::DateTime::parse_from_rfc3339(s)
            .map(|dt| CredentialExpiry::Fixed(dt.with_timezone(&chrono::Utc)))
            .map_err(|e| {
                ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                    .title("Invalid Template Expiration")
                    .message(format!("Template expiration datetime `{s}` could not be parsed: {e}"))
                    .finish()
            }),
        Expiration::Duration(s) => iso8601::duration(s)
            .map_err(|_| {
                ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                    .title("Invalid Template Expiration")
                    .message(format!("Template expiration duration `{s}` could not be parsed"))
                    .finish()
            })
            .map(|d| {
                let delta = match d {
                    iso8601::Duration::YMDHMS {
                        year,
                        month,
                        day,
                        hour,
                        minute,
                        second,
                        millisecond,
                    } => {
                        chrono::Duration::days(year as i64 * 365 + month as i64 * 30 + day as i64)
                            + chrono::Duration::hours(hour as i64)
                            + chrono::Duration::minutes(minute as i64)
                            + chrono::Duration::seconds(second as i64)
                            + chrono::Duration::milliseconds(millisecond as i64)
                    }
                    iso8601::Duration::Weeks(w) => chrono::Duration::weeks(w as i64),
                };
                CredentialExpiry::Fixed(chrono::Utc::now() + delta)
            }),
    }
}

/// Validates that an explicitly provided `expires_at` does not exceed the template's deadline.
///
/// - If the template deadline is `Never`, any explicit value is accepted.
/// - If the explicit value is `Never` but the template has a fixed deadline, that is rejected.
/// - Otherwise, the explicit datetime must be ≤ the template deadline.
#[allow(clippy::result_large_err)]
fn validate_expiry_within_template_deadline(
    explicit: &CredentialExpiry,
    deadline: &CredentialExpiry,
) -> Result<(), ApiError> {
    match (explicit, deadline) {
        // Template has no deadline — anything goes.
        (_, CredentialExpiry::Never) => Ok(()),
        // Explicit is "never" but template enforces a deadline.
        (CredentialExpiry::Never, CredentialExpiry::Fixed(limit)) => Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Expiration Exceeds Template Limit")
            .type_url(type_url("issuance#expiration-exceeds-template-limit"))
            .message(format!(
                "The template requires an expiration date not after {}",
                limit.to_rfc3339()
            ))
            .finish()),
        // Both are fixed — compare them.
        (CredentialExpiry::Fixed(requested), CredentialExpiry::Fixed(limit)) => {
            if requested > limit {
                Err(ApiError::builder(StatusCode::BAD_REQUEST)
                    .title("Expiration Exceeds Template Limit")
                    .type_url(type_url("issuance#expiration-exceeds-template-limit"))
                    .message(format!(
                        "The template requires an expiration date not after {}",
                        limit.to_rfc3339()
                    ))
                    .finish())
            } else {
                Ok(())
            }
        }
    }
}

/// Validates the credential data against the template's JSON Schema.///
/// Returns a detailed error response if validation fails, listing all schema violations.
fn validate_credential_against_schema(credential: &Value, schema: &Value) -> Result<(), Box<ApiError>> {
    let validator = jsonschema::validator_for(schema).map_err(|e| {
        Box::new(
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Invalid Template Schema")
                .type_url(type_url("issuance#invalid-template-schema"))
                .message(format!("The template's schema is not a valid JSON Schema: {e}"))
                .finish(),
        )
    })?;

    let (violations, descriptions): (Vec<SchemaViolation>, Vec<String>) = validator
        .iter_errors(credential)
        .map(|e| {
            (
                SchemaViolation {
                    path: e.instance_path().to_string(),
                    message: e.to_string(),
                },
                format!("Path `{}`: {} (schema path: {})", e.instance_path(), e, e.schema_path()),
            )
        })
        .unzip();

    if violations.is_empty() {
        Ok(())
    } else {
        Err(Box::new(
            ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Credential Schema Validation Failed")
                .type_url(type_url("issuance#credential-schema-validation-failed"))
                .message(format!(
                    "The credential does not match the template schema. Violations:\n{}",
                    descriptions
                        .iter()
                        .enumerate()
                        .map(|(i, e)| format!("  [{}] {}", i + 1, e))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
                .field("violations", &violations)
                .finish(),
        ))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::tests::{OFFER_ID, TEMPLATE_ID};
    use crate::v0::issuance::{credential_issuer::token_status_list::tests::create_test_signed_credential, router};
    use crate::API_VERSION;
    use agent_issuance::application::credential_configuration_projection::CredentialConfigurationProjection;
    use agent_issuance::issuance_state;
    use agent_issuance::offer::{aggregate::Offer, error::OfferError};
    use agent_issuance::{services::IssuanceServices, state::initialize};
    use agent_library::library_state;
    use agent_library::template::aggregate::{DataModel, Display, Expiration, HolderType, Status, Visibility};
    use agent_library::template::command::TemplateCommand;
    use agent_secret_manager::service::Service;
    use agent_secret_manager::subject::Subject;
    use agent_shared::application_state::{Command, CommandHandler};
    use agent_shared::config::TESTINDEX;
    use agent_store::in_memory::InMemory;
    use axum::{
        body::{self, Body},
        http::{self, Request, StatusCode},
        Router,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use cqrs_es::AggregateError;
    use lazy_static::lazy_static;
    use oauth_tsl::relying_party::check_status_in_status_list_token_jwt;
    use oauth_tsl::relying_party::{decompress_gzip, StatusListTokenResponseType};
    use serde_json::json;
    use shared_kernel::authorization::{
        AuthorizationChecker, AuthorizationError, AuthorizationOperation, AuthorizationRequest,
    };
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
            "id": "urn:uuid:123e4567-e89b-12d3-a456-426614174000",
            "@context": [
                "https://www.w3.org/2018/credentials/v1",
                {
                    "logo_uri": {
                        "@id": "https://www.iana.org/assignments/jwt#logo_uri",
                        "@type": "@id"
                    }
                }
            ],
            "type": [ "VerifiableCredential" ],
            "name": "Verifiable Credential",
            "issuer": {
                "name": "UniCore"
            },
            "credentialSubject": CREDENTIAL_SUBJECT.clone(),
        });
    }

    /// Creates a [LibraryState] with the [CredentialConfigurationProjection] properly wired
    /// to the given [IssuanceState], mirroring the production application setup. When a template
    /// is created or updated through this library state, the credential configuration in the
    /// issuance state is automatically synchronized via the projection.
    pub async fn setup_library_state(issuance_state: &Arc<IssuanceState>) -> Arc<LibraryState> {
        let (projection, view_handle) = CredentialConfigurationProjection::new(issuance_state.clone());
        let event_bus = shared_kernel::event_bus::EventBusHandle::default();
        let lib = Arc::new(library_state(&InMemory, &event_bus, vec![Box::new(projection)]).await);
        assert!(
            view_handle.set(lib.query.template.clone()).is_ok(),
            "template view already initialized"
        );
        lib
    }

    pub async fn create_new_template(
        library_state: &Arc<LibraryState>,
        status: Status,
        credential_expiration: Option<Expiration>,
        pre_authorized: bool,
        data_model: DataModel,
    ) -> String {
        let mut library_app = crate::v0::library::router(library_state.clone());
        let response = library_app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/create-new-template"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "title": "Test Template",
                            "display": {
                                "name": "Verifiable Credential",
                                "logo": {
                                    "uri": "https://www.impierce.com/external/impierce-logo.png",
                                    "altText": "Impierce Logo"
                                }
                            },
                            "dataModel": data_model,
                            "holderType": HolderType::Individual,
                            "status": status,
                            "visibility": Visibility::Private,
                            "credentialExpiration": credential_expiration,
                            "type": ["VerifiableCredential"],
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "first_name": { "type": "string" },
                                    "last_name": { "type": "string" }
                                },
                                "required": ["first_name", "last_name"]
                            },
                            "holderAuthorization": {
                                "pre_authorized": pre_authorized
                            }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        body["id"].as_str().unwrap().to_string()
    }

    async fn update_template_status(library_state: &Arc<LibraryState>, template_id: &str, status: Status) {
        let mut library_app = crate::v0::library::router(library_state.clone());
        let response = library_app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/update-template"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "id": template_id,
                            "status": status
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    async fn delete_template(library_state: &Arc<LibraryState>, template_id: &str) {
        let mut library_app = crate::v0::library::router(library_state.clone());
        let response = library_app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/delete-template"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "id": template_id
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    /// Creates a test template in the library state and returns its ID.
    /// The template will be automatically synchronized as a credential configuration in the
    /// issuance state if the library state was created with [setup_library_with_projection].
    /// The `pre_authorized` parameter controls the authorization flow type for the credential configuration.
    pub async fn create_test_template_with_auth(library_state: &Arc<LibraryState>, pre_authorized: bool) -> String {
        let template_id = TEMPLATE_ID.to_string();

        command_handler(
            library_state.authorization_checker.clone(),
            None,
            &template_id,
            &library_state.command.template,
            TemplateCommand::CreateNewTemplate {
                template_id: template_id.clone(),
                source_template_id: None,
                title: "Test Template".to_string(),
                display: Box::new(Some(Display {
                    name: "Verifiable Credential".to_string(),
                    logo: None,
                })),
                data_model: DataModel::W3CVcDataModelV1_1,
                holder_type: HolderType::Individual,
                tags: None,
                status: Status::Published,
                visibility: Visibility::Private,
                credential_expiration: Some(Expiration::Never),
                description: None,
                r#type: vec!["VerifiableCredential".to_string()],
                schema: Box::new(Some(json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "first_name": { "type": "string" },
                        "last_name": { "type": "string" }
                    },
                    "required": ["first_name", "last_name"]
                }))),
                schema_properties_attributes: None,
                holder_authorization: agent_shared::config::Authorization {
                    pre_authorized,
                    tx_code_constraints: None,
                },
            },
        )
        .await
        .unwrap();

        template_id
    }

    /// Creates a test template with pre-authorized credential configuration (default).
    pub async fn create_test_template(library_state: &Arc<LibraryState>) -> String {
        create_test_template_with_auth(library_state, true).await
    }

    pub async fn create_test_template_with_status_and_format(
        library_state: &Arc<LibraryState>,
        status: Status,
        credential_expiration: Option<Expiration>,
        format: &str,
    ) -> String {
        let data_model = match format {
            "jwt_vc_json" => DataModel::W3CVcDataModelV1_1,
            "vc+sd-jwt" => DataModel::W3CVcDataModelV2_0,
            _ => panic!("unsupported test format: {format}"),
        };
        let initial_status = match status.clone() {
            Status::Archived | Status::Deleted => Status::Draft,
            _ => status.clone(),
        };

        let template_id =
            create_new_template(library_state, initial_status, credential_expiration, true, data_model).await;

        match status {
            Status::Archived => update_template_status(library_state, &template_id, Status::Archived).await,
            Status::Deleted => delete_template(library_state, &template_id).await,
            _ => {}
        }

        template_id
    }

    fn encode_base64url_json(value: Value) -> String {
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).unwrap())
    }

    /// No _valid_ signature is produced, which is sufficient for the testing purpose.
    fn build_test_signed_jwt_vc_json() -> String {
        let header = encode_base64url_json(json!({
            "alg": "EdDSA",
            "typ": "JWT"
        }));
        let payload = encode_base64url_json(json!({
            "iss": "did:example:issuer",
            "sub": "did:example:holder",
            "vc": {
                "@context": ["https://www.w3.org/2018/credentials/v1"],
                "type": ["VerifiableCredential"],
                "credentialSubject": {
                    "id": "did:example:holder"
                }
            }
        }));
        let signature = URL_SAFE_NO_PAD.encode(b"signature");

        format!("{header}.{payload}.{signature}")
    }

    pub async fn signed_credentials_with_template(
        app: &mut Router,
        template_id: &str,
        signed_credential: &str,
        expires_at: Option<Value>,
    ) -> Response {
        let mut request_body = json!({
            "templateId": template_id,
            "offerId": OFFER_ID,
            "credential": signed_credential,
            "isSigned": true,
        });

        if let Some(expires_at) = expires_at {
            request_body["expiresAt"] = expires_at;
        }

        app.call(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("{API_VERSION}/credentials"))
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
    }

    pub async fn unsigned_credentials_request_with_template(app: &mut Router, template_id: &str) -> Response {
        app.call(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("{API_VERSION}/credentials"))
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "templateId": template_id,
                        "offerId": OFFER_ID,
                        "credential": {
                            "credentialSubject": CREDENTIAL_SUBJECT.clone(),
                        },
                        "expiresAt": "never"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
    }

    /// This function creates and tests a credential and returns the endpoint where this credential can be accessed.
    pub async fn credentials(app: &mut Router) -> String {
        credentials_with_template(app, TEMPLATE_ID).await
    }

    /// This function creates and tests a credential with a specific template ID and returns the endpoint where this credential can be accessed.
    pub async fn credentials_with_template(app: &mut Router, template_id: &str) -> String {
        let response = unsigned_credentials_request_with_template(app, template_id).await;

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
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;

        let mut app = router((issuance_state.clone(), library_state));

        let credential_endpoint = create_test_signed_credential(&mut app, &issuance_state, &template_id).await;
        patch_credential(&mut app, credential_endpoint).await;
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_credentials_endpoint() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;

        let mut app = router((issuance_state.clone(), library_state));

        credentials_with_template(&mut app, &template_id).await;
    }

    #[tokio::test]
    async fn test_credentials_endpoint_requires_template_id() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let mut app = router((issuance_state.clone(), library_state));

        let response = unsigned_credentials_request_with_template(&mut app, "").await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["title"], "Missing Template ID");
    }

    #[tokio::test]
    async fn test_credentials_endpoint_requires_existing_template() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let mut app = router((issuance_state.clone(), library_state));

        let response = unsigned_credentials_request_with_template(&mut app, "missing-template").await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["title"], "Template Not Found");
    }

    #[tokio::test]
    async fn test_credentials_endpoint_requires_credential_configuration() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        let template_id = create_new_template(
            &library_state,
            Status::Published,
            Some(Expiration::Never),
            true,
            DataModel::W3CVcDataModelV1_1,
        )
        .await;

        let mut app = router((issuance_state.clone(), library_state));
        let response = unsigned_credentials_request_with_template(&mut app, &template_id).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["title"], "No Credential Configuration Found");
    }

    #[tokio::test]
    async fn test_signed_credentials_require_published_template() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template_with_status_and_format(
            &library_state,
            Status::Draft,
            Some(Expiration::Never),
            "jwt_vc_json",
        )
        .await;

        let mut app = router((issuance_state.clone(), library_state));
        let response = signed_credentials_with_template(
            &mut app,
            &template_id,
            &build_test_signed_jwt_vc_json(),
            Some(json!("never")),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["title"], "Template Not Published");
    }

    #[tokio::test]
    async fn test_unsigned_credentials_require_published_template() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let draft_template_id = create_test_template_with_status_and_format(
            &library_state,
            Status::Draft,
            Some(Expiration::Never),
            "jwt_vc_json",
        )
        .await;
        let archived_template_id = create_test_template_with_status_and_format(
            &library_state,
            Status::Archived,
            Some(Expiration::Never),
            "jwt_vc_json",
        )
        .await;

        let mut app = router((issuance_state.clone(), library_state));

        for template_id in [&draft_template_id, &archived_template_id] {
            let response = unsigned_credentials_request_with_template(&mut app, template_id).await;

            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["title"], "Template Not Published");
        }
    }

    #[tokio::test]
    async fn test_signed_credentials_ignore_expires_at_request_override() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template_with_status_and_format(
            &library_state,
            Status::Published,
            Some(Expiration::DateTime("2000-01-01T00:00:00Z".to_string())),
            "jwt_vc_json",
        )
        .await;

        let mut app = router((issuance_state.clone(), library_state));
        let signed_credential = build_test_signed_jwt_vc_json();
        let response =
            signed_credentials_with_template(&mut app, &template_id, &signed_credential, Some(json!("never"))).await;

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body, json!(signed_credential));
    }

    #[tokio::test]
    async fn test_signed_credentials_must_match_template_configuration_format() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template_with_status_and_format(
            &library_state,
            Status::Published,
            Some(Expiration::Never),
            "vc+sd-jwt",
        )
        .await;

        let mut app = router((issuance_state.clone(), library_state));
        let response =
            signed_credentials_with_template(&mut app, &template_id, &build_test_signed_jwt_vc_json(), None).await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["title"], "Signed Credential Format Mismatch");
    }

    #[tokio::test]
    async fn test_all_credentials_lists_created_credentials_and_rejects_invalid_ones() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state.clone(), library_state));

        async fn send(app: &mut Router, method: http::Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
            let response = app
                .call(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
            (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
        }
        let all_credentials = format!("{API_VERSION}/credentials");

        assert_eq!(
            send(&mut app, http::Method::GET, &all_credentials, None).await,
            (StatusCode::OK, json!([]))
        );

        // Signed credentials must be strings.
        let (status, body) = send(
            &mut app,
            http::Method::POST,
            &all_credentials,
            Some(json!({
                "templateId": template_id,
                "offerId": OFFER_ID,
                "credential": { "credentialSubject": CREDENTIAL_SUBJECT.clone() },
                "isSigned": true,
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["title"], "Invalid Credential Type");

        // Without `expiresAt`, the template's credential expiration applies.
        let (status, _) = send(
            &mut app,
            http::Method::POST,
            &all_credentials,
            Some(json!({
                "templateId": template_id,
                "offerId": OFFER_ID,
                "credential": { "credentialSubject": CREDENTIAL_SUBJECT.clone() },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let credential_endpoint = credentials_with_template(&mut app, &template_id).await;

        let (status, credentials) = send(&mut app, http::Method::GET, &all_credentials, None).await;
        assert_eq!(status, StatusCode::OK);
        let credentials = credentials.as_array().unwrap();
        assert_eq!(credentials.len(), 2);
        let (_, newest) = send(&mut app, http::Method::GET, &credential_endpoint, None).await;
        assert_eq!(credentials[0], newest, "credentials are listed newest first");

        // Both credentials were added to the same offer.
        let offer = offer_view(&issuance_state, OFFER_ID).await.unwrap();
        assert_eq!(offer.credential_ids.len(), 2);

        let (status, _) = send(
            &mut app,
            http::Method::PATCH,
            &format!("{API_VERSION}/credentials/unknown"),
            Some(json!({ "credentialStatus": "INVALID" })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    const VERIFY_BATCH: &str = "/verify-credentials-batch";
    const CREATE_BATCH: &str = "/create-credentials-batch";

    pub async fn batch_request(app: &mut Router, path: &str, request: &Value) -> Response {
        app.call(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("{API_VERSION}{path}"))
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(Body::from(serde_json::to_vec(request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
    }

    fn valid_row() -> Value {
        json!({ "claims": CREDENTIAL_SUBJECT.clone() })
    }

    async fn json_body(response: Response) -> Value {
        serde_json::from_slice(&body::to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

    async fn offer_view(
        issuance_state: &Arc<IssuanceState>,
        offer_id: &str,
    ) -> Option<agent_issuance::offer::views::OfferView> {
        query_handler(
            issuance_state.authorization_checker.clone(),
            None,
            offer_id,
            Some(offer_id),
            &issuance_state.query.offer,
        )
        .await
        .unwrap()
    }

    async fn credential_count(app: &mut Router) -> usize {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("{API_VERSION}/credentials"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        json_body(response).await.as_array().unwrap().len()
    }

    /// Asserts a row was created and that its IDs address a credential that sits in its offer.
    async fn assert_created(app: &mut Router, issuance_state: &Arc<IssuanceState>, result: &Value, index: usize) {
        assert_eq!(result["index"], index);
        assert_eq!(result["status"], 201);
        assert!(result.get("error").is_none());

        let credential_id = result["credentialId"].as_str().unwrap();
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("{API_VERSION}/credentials/{credential_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let offer = offer_view(issuance_state, result["offerId"].as_str().unwrap())
            .await
            .unwrap();
        assert_eq!(offer.credential_ids, vec![credential_id.to_string()]);
        assert_eq!(
            result["credentialOffer"].as_str(),
            offer.form_url_encoded_credential_offer.as_deref()
        );
        assert!(result["credentialOffer"]
            .as_str()
            .unwrap()
            .starts_with("openid-credential-offer://"));
    }

    /// Denies one operation and allows every other one.
    struct DenyOperation(&'static str);

    #[async_trait::async_trait]
    impl AuthorizationChecker for DenyOperation {
        async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            let operation_name = match &request.operation {
                AuthorizationOperation::Command { operation_name, .. }
                | AuthorizationOperation::Query { operation_name, .. } => operation_name,
            };

            if *operation_name == self.0 {
                Err(AuthorizationError::Forbidden)
            } else {
                Ok(())
            }
        }
    }

    /// Fails the n-th `AddCredentials`, so a batch breaks part-way through one row.
    struct FailNthAddCredentials {
        inner: CommandHandler<Offer>,
        fail_at: usize,
        seen: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Command<Offer> for FailNthAddCredentials {
        async fn execute_with_metadata(
            &self,
            aggregate_id: &str,
            command: OfferCommand,
            metadata: std::collections::HashMap<String, String>,
        ) -> Result<(), AggregateError<OfferError>> {
            if matches!(command, OfferCommand::AddCredentials { .. })
                && self.seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 == self.fail_at
            {
                return Err(AggregateError::AggregateConflict);
            }

            self.inner.execute_with_metadata(aggregate_id, command, metadata).await
        }
    }

    #[tokio::test]
    async fn test_batch_rejects_empty_and_oversized_batches() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state, library_state));

        for path in [VERIFY_BATCH, CREATE_BATCH] {
            let response =
                batch_request(&mut app, path, &json!({ "templateId": template_id, "credentials": [] })).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(json_body(response).await["title"], "Empty Batch");

            let rows: Vec<Value> = (0..=MAX_BATCH_SIZE).map(|_| valid_row()).collect();
            let response = batch_request(
                &mut app,
                path,
                &json!({ "templateId": template_id, "credentials": rows }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(json_body(response).await["title"], "Batch Too Large");
        }
    }

    #[tokio::test]
    async fn test_batch_template_problems_fail_the_whole_request() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let draft_template_id = create_test_template_with_status_and_format(
            &library_state,
            Status::Draft,
            Some(Expiration::Never),
            "jwt_vc_json",
        )
        .await;
        let mut app = router((issuance_state, library_state));

        for path in [VERIFY_BATCH, CREATE_BATCH] {
            let response = batch_request(
                &mut app,
                path,
                &json!({ "templateId": "missing-template", "credentials": [valid_row()] }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert_eq!(json_body(response).await["title"], "Template Not Found");

            let response = batch_request(
                &mut app,
                path,
                &json!({ "templateId": draft_template_id, "credentials": [valid_row()] }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(json_body(response).await["title"], "Template Not Published");
        }
    }

    #[tokio::test]
    async fn test_verify_batch_reports_every_row_and_creates_nothing() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state.clone(), library_state));

        let response = batch_request(
            &mut app,
            VERIFY_BATCH,
            &json!({
                "templateId": template_id,
                "credentials": [
                    valid_row(),
                    { "claims": { "first_name": "Ferris" }, "recipientEmail": "ferris@example.org" },
                    { "claims": "not an object" },
                ]
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let results = json_body(response).await;

        assert_eq!(results[0], json!({ "index": 0, "status": 200 }));

        assert_eq!(results[1]["index"], 1);
        assert_eq!(results[1]["status"], 422);
        assert_eq!(results[1]["error"]["title"], "Credential Schema Validation Failed");
        assert_eq!(
            results[1]["error"]["type"],
            type_url("issuance#credential-schema-validation-failed")
        );
        let violations = results[1]["error"]["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0]["path"], "");
        assert!(violations[0]["message"].as_str().unwrap().contains("last_name"));

        assert_eq!(results[2]["index"], 2);
        assert_eq!(results[2]["status"], 400);
        assert_eq!(results[2]["error"]["title"], "Invalid Credential Type");
        assert_eq!(results[2]["error"]["detail"], "`claims` must be an object.");

        assert_eq!(credential_count(&mut app).await, 0);
    }

    #[tokio::test]
    async fn test_verify_batch_reports_forbidden_commands() {
        let mut issuance_state =
            issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;
        issuance_state.authorization_checker = Arc::new(DenyOperation("issuance.offers.send"));
        let issuance_state = Arc::new(issuance_state);
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state.clone(), library_state));

        // Sending is only authorized when a row asks for an email.
        let response = batch_request(
            &mut app,
            VERIFY_BATCH,
            &json!({ "templateId": template_id, "credentials": [valid_row()] }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = batch_request(
            &mut app,
            VERIFY_BATCH,
            &json!({
                "templateId": template_id,
                "credentials": [{ "claims": CREDENTIAL_SUBJECT.clone(), "recipientEmail": "ferris@example.org" }]
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_body(response).await["title"], "Forbidden");
        assert_eq!(credential_count(&mut app).await, 0);
    }

    #[tokio::test]
    async fn test_create_batch_creates_one_credential_and_offer_per_row() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state.clone(), library_state));

        let response = batch_request(
            &mut app,
            CREATE_BATCH,
            &json!({
                "templateId": template_id,
                "expiresAt": "never",
                "credentials": [
                    valid_row(),
                    { "claims": CREDENTIAL_SUBJECT.clone(), "recipientEmail": "ferris@example.org" },
                    { "claims": CREDENTIAL_SUBJECT.clone(), "recipientEmail": "   " },
                ]
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = json_body(response).await;
        let results = body.as_array().unwrap();
        assert_eq!(results.len(), 3);

        for (index, result) in results.iter().enumerate() {
            assert_created(&mut app, &issuance_state, result, index).await;
        }
        assert_eq!(results[0]["emailRequested"], false);
        assert_eq!(results[1]["emailRequested"], true);
        assert_eq!(results[2]["emailRequested"], false, "a blank address is no address");

        // Every row got its own offer.
        let offer_ids: std::collections::HashSet<&str> = results
            .iter()
            .map(|result| result["offerId"].as_str().unwrap())
            .collect();
        assert_eq!(offer_ids.len(), 3);

        // The claims were wrapped for the template's W3C data model.
        let credential_id = results[0]["credentialId"].as_str().unwrap();
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("{API_VERSION}/credentials/{credential_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            json_body(response).await["data"]["raw"]["credentialSubject"],
            CREDENTIAL_SUBJECT.clone()
        );
    }

    #[tokio::test]
    async fn test_create_batch_creates_valid_rows_and_reports_invalid_ones() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state.clone(), library_state));

        let response = batch_request(
            &mut app,
            CREATE_BATCH,
            &json!({
                "templateId": template_id,
                "credentials": [
                    { "claims": { "wrong_field": "data" } },
                    valid_row(),
                ]
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::MULTI_STATUS);
        let body = json_body(response).await;
        let results = body.as_array().unwrap();
        assert_eq!(results.len(), 2);

        assert_eq!(results[0]["index"], 0);
        assert_eq!(results[0]["status"], 422);
        assert!(results[0].get("credentialId").is_none());
        assert!(results[0].get("offerId").is_none());
        assert_eq!(results[0]["error"]["title"], "Credential Schema Validation Failed");
        assert_eq!(results[0]["error"]["violations"].as_array().unwrap().len(), 2);

        assert_created(&mut app, &issuance_state, &results[1], 1).await;
        assert_eq!(credential_count(&mut app).await, 1);
    }

    #[tokio::test]
    async fn test_create_batch_reports_what_a_failing_row_had_created() {
        let mut issuance_state =
            issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;
        issuance_state.command.offer = Arc::new(FailNthAddCredentials {
            inner: issuance_state.command.offer.clone(),
            fail_at: 2,
            seen: Default::default(),
        });
        let issuance_state = Arc::new(issuance_state);
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state.clone(), library_state));

        let response = batch_request(
            &mut app,
            CREATE_BATCH,
            &json!({ "templateId": template_id, "credentials": [valid_row(), valid_row(), valid_row()] }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::MULTI_STATUS);
        let body = json_body(response).await;
        let results = body.as_array().unwrap();
        assert_eq!(results.len(), 3);

        assert_created(&mut app, &issuance_state, &results[0], 0).await;
        assert_created(&mut app, &issuance_state, &results[2], 2).await;

        // Row 1 created its credential and offer, then failed to add one to the other.
        assert_eq!(results[1]["index"], 1);
        assert_eq!(results[1]["status"], 503);
        assert!(results[1]["credentialId"].is_string());
        assert!(results[1]["offerId"].is_string());
        assert!(results[1].get("credentialOffer").is_none());
        assert_eq!(results[1]["error"]["title"], "Aggregate Conflict");
        assert_eq!(results[1]["error"]["type"], type_url("persistence#aggregate-conflict"));
        let orphaned_offer = offer_view(&issuance_state, results[1]["offerId"].as_str().unwrap())
            .await
            .unwrap();
        assert!(orphaned_offer.credential_ids.is_empty());
    }

    #[tokio::test]
    async fn test_create_batch_with_no_valid_row_is_a_problem() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;
        let mut app = router((issuance_state.clone(), library_state));

        let response = batch_request(
            &mut app,
            CREATE_BATCH,
            &json!({
                "templateId": template_id,
                "credentials": [{ "claims": {} }, { "claims": [] }]
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
        let body = json_body(response).await;
        assert_eq!(body["type"], type_url("issuance#batch-failed"));
        assert_eq!(body["title"], "Batch Failed");
        assert_eq!(body["status"], 422);
        assert_eq!(body["detail"], "None of the 2 credentials could be created.");

        let results = body["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["status"], 422);
        assert_eq!(results[0]["error"]["title"], "Credential Schema Validation Failed");
        assert_eq!(results[1]["status"], 400);
        assert_eq!(results[1]["error"]["title"], "Invalid Credential Type");
        assert_eq!(credential_count(&mut app).await, 0);
    }

    mod expiration_to_credential_expiry_tests {
        use super::*;
        use agent_library::template::aggregate::Expiration;

        #[test]
        fn never_maps_to_never() {
            let result = expiration_to_credential_expiry(&Expiration::Never).unwrap();
            assert!(matches!(result, CredentialExpiry::Never));
        }

        #[test]
        fn datetime_maps_to_fixed() {
            let result =
                expiration_to_credential_expiry(&Expiration::DateTime("2030-06-01T00:00:00Z".to_string())).unwrap();
            match result {
                CredentialExpiry::Fixed(dt) => {
                    assert_eq!(dt.to_rfc3339(), "2030-06-01T00:00:00+00:00");
                }
                _ => panic!("expected Fixed"),
            }
        }

        #[test]
        fn invalid_datetime_returns_error() {
            let result = expiration_to_credential_expiry(&Expiration::DateTime("not-a-date".to_string()));
            assert!(result.is_err());
            assert_eq!(result.unwrap_err().status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[test]
        fn duration_days_maps_to_fixed_in_the_future() {
            let before = chrono::Utc::now();
            let result = expiration_to_credential_expiry(&Expiration::Duration("P30D".to_string())).unwrap();
            let after = chrono::Utc::now();

            match result {
                CredentialExpiry::Fixed(dt) => {
                    let thirty_days = chrono::Duration::days(30);
                    assert!(dt >= before + thirty_days);
                    assert!(dt <= after + thirty_days);
                }
                _ => panic!("expected Fixed"),
            }
        }

        #[test]
        fn duration_weeks_maps_to_fixed_in_the_future() {
            let before = chrono::Utc::now();
            let result = expiration_to_credential_expiry(&Expiration::Duration("P2W".to_string())).unwrap();
            let after = chrono::Utc::now();

            match result {
                CredentialExpiry::Fixed(dt) => {
                    let two_weeks = chrono::Duration::weeks(2);
                    assert!(dt >= before + two_weeks);
                    assert!(dt <= after + two_weeks);
                }
                _ => panic!("expected Fixed"),
            }
        }

        #[test]
        fn invalid_duration_returns_error() {
            let result = expiration_to_credential_expiry(&Expiration::Duration("not-a-duration".to_string()));
            assert!(result.is_err());
            assert_eq!(result.unwrap_err().status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    mod validate_expiry_within_template_deadline_tests {
        use super::*;

        fn fixed(rfc3339: &str) -> CredentialExpiry {
            CredentialExpiry::Fixed(
                chrono::DateTime::parse_from_rfc3339(rfc3339)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            )
        }

        #[test]
        fn never_deadline_accepts_never() {
            let result = validate_expiry_within_template_deadline(&CredentialExpiry::Never, &CredentialExpiry::Never);
            assert!(result.is_ok());
        }

        #[test]
        fn never_deadline_accepts_fixed() {
            let result =
                validate_expiry_within_template_deadline(&fixed("2030-01-01T00:00:00Z"), &CredentialExpiry::Never);
            assert!(result.is_ok());
        }

        #[test]
        fn never_explicit_with_fixed_deadline_is_rejected() {
            let result =
                validate_expiry_within_template_deadline(&CredentialExpiry::Never, &fixed("2030-01-01T00:00:00Z"));
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert_eq!(err.status(), StatusCode::BAD_REQUEST);
            assert!(err
                .message()
                .unwrap_or_default()
                .contains("The template requires an expiration date not after"));
        }

        #[test]
        fn fixed_within_deadline_is_accepted() {
            let result = validate_expiry_within_template_deadline(
                &fixed("2029-06-01T00:00:00Z"),
                &fixed("2030-01-01T00:00:00Z"),
            );
            assert!(result.is_ok());
        }

        #[test]
        fn fixed_equal_to_deadline_is_accepted() {
            let result = validate_expiry_within_template_deadline(
                &fixed("2030-01-01T00:00:00Z"),
                &fixed("2030-01-01T00:00:00Z"),
            );
            assert!(result.is_ok());
        }

        #[test]
        fn fixed_exceeding_deadline_is_rejected() {
            let result = validate_expiry_within_template_deadline(
                &fixed("2031-01-01T00:00:00Z"),
                &fixed("2030-01-01T00:00:00Z"),
            );
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert_eq!(err.status(), StatusCode::BAD_REQUEST);
            assert!(err
                .message()
                .unwrap_or_default()
                .contains("The template requires an expiration date not after 2030-01-01T00:00:00+00:00"));
        }
    }
}

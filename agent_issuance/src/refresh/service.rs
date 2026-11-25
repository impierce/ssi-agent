use crate::{
    credential::{aggregate::Status, views::CredentialView},
    refresh::{
        aggregate::{Continuation, RefreshCapability},
        command::RefreshCapabilityCommand,
        error::RefreshCapabilityError,
        preparation::RefreshPreparationError,
        token::RefreshToken,
    },
    reissuance::{error::ReissuanceError, service::reissue_credential},
    state::IssuanceState,
};
use agent_library::state::LibraryState;
use agent_shared::handlers::{public_command_handler, public_query_handler, CommandHandlerError};
use cqrs_es::AggregateError;
use oid4vci::notification_request::NotificationEvent;
use shared_kernel::authorization::Caller;
use tracing::{error, info};

/// Why a refresh produced no offer. Failures are logged with their cause where they happen; the variants only
/// carry what the holder is told.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RefreshError {
    /// The token is unknown, or its capability is retired. The two are indistinguishable on purpose.
    #[error("No refreshable credential was found for the refresh token")]
    NotFound,
    /// The preparation hook or the reissuance policy refused the refresh.
    #[error("The credential cannot be refreshed")]
    Unavailable,
    /// The preparation hook failed.
    #[error("Preparing the replacement credential failed")]
    PreparationFailed,
    /// Any other failure.
    #[error("The credential refresh failed")]
    Failed,
}

/// Answers a holder's refresh request with the form-url-encoded credential offer of a replacement credential.
///
/// - **Unknown or retired token:** [`RefreshError::NotFound`].
/// - **Retirement is lazy:** once the holder accepted (`credential_accepted`) any replacement made from the
///   capability, this call records the retirement and answers `NotFound`. Nothing retires capabilities in the
///   background, and a missing acceptance report means "receipt unknown", so the token stays usable.
/// - **Pending offer reuse:** while the newest replacement has not been delivered (its credential is not issued
///   yet), the answer is that replacement's offer, and the preparation hook is not asked again.
/// - **Otherwise** the hook prepares a replacement, which is reissued from the credential that carries the token.
///   The original and its status are left as they are; a replacement whose configuration opts into refresh gets a
///   capability of its own.
/// - **Order of writes:** the replacement (relation, credential, offer) first, recording it as the capability's
///   pending continuation last. Any failure before that records nothing, so a retry starts clean and leftovers are
///   never handed out. When a concurrent call records first, this call answers with that call's offer.
/// - **Leftovers:** a call that fails halfway, or loses to a concurrent call, leaves the relation, credential and offer
///   it wrote behind. They are never handed out, but the relation does show in the reissuance listing.
pub async fn refresh_credential(
    state: &IssuanceState,
    library_state: &LibraryState,
    token: &RefreshToken,
) -> Result<String, RefreshError> {
    let capability_id = token.capability_id();
    let capability = active_capability(state, &capability_id).await?;

    if let Some(accepted) = accepted_continuation(state, &capability).await? {
        public_command_handler(
            &capability_id,
            &state.command.refresh_capability,
            RefreshCapabilityCommand::RetireRefreshCapability {
                accepted_credential_id: accepted.credential_id.clone(),
            },
        )
        .await
        .map_err(|failure| failed("Failed to retire a refresh capability", &failure))?;
        info!(
            capability_id,
            "Retired a refresh capability after its replacement was accepted"
        );
        return Err(RefreshError::NotFound);
    }

    if let Some(offer) = pending_offer(state, &capability).await? {
        return Ok(offer);
    }

    let original = public_query_handler(&capability.credential_id, &state.query.credential)
        .await
        .map_err(|failure| failed("Failed to load the credential to refresh", &failure))?
        .ok_or_else(|| failed("The credential to refresh is missing", &capability.credential_id))?;
    let replacement = state
        .ports
        .refresh_preparation
        .prepare(&original, &capability.credential_configuration_id)
        .await
        .map_err(|refusal| match refusal {
            RefreshPreparationError::Refused => RefreshError::Unavailable,
            RefreshPreparationError::Failed(failure) => {
                error!(error = %failure, capability_id, "Failed to prepare a replacement credential");
                RefreshError::PreparationFailed
            }
        })?;

    let prepared = reissue_credential(
        state,
        library_state,
        Caller::Internal,
        &capability.credential_id,
        replacement,
    )
    .await
    .map_err(|failure| match failure {
        ReissuanceError::NotAllowed(_) => RefreshError::Unavailable,
        ReissuanceError::Persistence(failure) => failed("Failed to reissue a refreshed credential", &failure),
        // Only the kind: the other reissuance errors can echo the replacement's claims.
        failure => failed("Failed to reissue a refreshed credential", &<&str>::from(&failure)),
    })?;

    let relation = prepared.relation;
    let recorded = public_command_handler(
        &capability_id,
        &state.command.refresh_capability,
        RefreshCapabilityCommand::RecordContinuation {
            after: capability
                .newest_continuation()
                .map(|continuation| continuation.reissuance_id.clone()),
            continuation: Continuation {
                reissuance_id: relation.reissuance_id,
                credential_id: relation.new_credential_id,
                offer_id: relation.offer_id,
            },
        },
    )
    .await;

    match recorded {
        Ok(()) => prepared
            .credential_offer
            .ok_or_else(|| failed("The offer of a replacement has no credential offer", &capability_id)),
        // Another refresh with the same token recorded its continuation first: answer with its offer.
        Err(CommandHandlerError::Aggregate(
            AggregateError::UserError(RefreshCapabilityError::ContinuationSuperseded)
            | AggregateError::AggregateConflict,
        )) => {
            let capability = active_capability(state, &capability_id).await?;
            pending_offer(state, &capability)
                .await?
                .ok_or_else(|| failed("A concurrent refresh left no pending offer", &capability_id))
        }
        Err(failure) => Err(failed("Failed to record a refresh continuation", &failure)),
    }
}

/// The capability of `capability_id`, unless it is unknown or retired.
async fn active_capability(state: &IssuanceState, capability_id: &str) -> Result<RefreshCapability, RefreshError> {
    public_query_handler(capability_id, &state.query.refresh_capability)
        .await
        .map_err(|failure| failed("Failed to load a refresh capability", &failure))?
        .filter(|capability| !capability.retired)
        .ok_or(RefreshError::NotFound)
}

/// The first continuation whose credential the holder reported as accepted.
async fn accepted_continuation<'a>(
    state: &IssuanceState,
    capability: &'a RefreshCapability,
) -> Result<Option<&'a Continuation>, RefreshError> {
    for continuation in &capability.continuations {
        let accepted = credential(state, &continuation.credential_id)
            .await?
            .holder_notifications
            .iter()
            .any(|notification| notification.event == NotificationEvent::CredentialAccepted);
        if accepted {
            return Ok(Some(continuation));
        }
    }
    Ok(None)
}

/// The offer of the newest continuation while its credential has not been delivered, as a form-url-encoded
/// credential offer.
async fn pending_offer(state: &IssuanceState, capability: &RefreshCapability) -> Result<Option<String>, RefreshError> {
    let Some(newest) = capability.newest_continuation() else {
        return Ok(None);
    };
    if credential(state, &newest.credential_id).await?.status == Status::Issued {
        return Ok(None);
    }
    public_query_handler(&newest.offer_id, &state.query.offer)
        .await
        .map_err(|failure| failed("Failed to load the pending offer of a refresh", &failure))?
        .and_then(|offer| offer.form_url_encoded_credential_offer)
        .map(Some)
        .ok_or_else(|| failed("The pending offer of a refresh is missing", &newest.reissuance_id))
}

async fn credential(state: &IssuanceState, credential_id: &str) -> Result<CredentialView, RefreshError> {
    public_query_handler(credential_id, &state.query.credential)
        .await
        .map_err(|failure| failed("Failed to load a replacement credential", &failure))?
        .ok_or_else(|| failed("A replacement credential is missing", &credential_id))
}

/// Logs `cause` and gives the holder's answer for any failure that is not the hook's.
fn failed(message: &str, cause: &dyn std::fmt::Display) -> RefreshError {
    error!(cause = %cause, "{message}");
    RefreshError::Failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::credential_creation::create_credential,
        credential::{aggregate::CredentialExpiry, command::CredentialCommand, entity::Data},
        refresh::preparation::RefreshPreparation,
        reissuance::service::{ReissuanceTrigger, Replacement},
        server_config::command::ServerConfigCommand,
        services::IssuanceServices,
        state::{initialize, issuance_state, SERVER_CONFIG_ID},
    };
    use agent_library::template::{
        aggregate::{Expiration, Status as TemplateStatus},
        command::TemplateCommand,
    };
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use oid4vci::notification_request::NotificationRequest;
    use serde_json::json;
    use shared_kernel::authorization::{
        AuthorizationChecker, AuthorizationError, AuthorizationOperation, AuthorizationRequest,
    };
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };

    const ORIGINAL: &str = "11111111-1111-4111-8111-111111111111";
    const CONFIGURATION: &str = "SD-JWT VC";

    /// Replaces every credential with a refreshed one of the same configuration, and counts how often it was asked.
    #[derive(Default)]
    struct RefreshEverything {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl RefreshPreparation for RefreshEverything {
        async fn prepare(&self, _: &Credential, _: &str) -> Result<Replacement, RefreshPreparationError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(refreshed())
        }
    }

    fn refreshed() -> Replacement {
        Replacement {
            credential_configuration_id: CONFIGURATION.to_owned(),
            credential: json!({"first_name": "Ferris", "last_name": "Refreshed"}),
            expires_at: Some(CredentialExpiry::Never),
            trigger: ReissuanceTrigger {
                trigger_type: Some("refresh".to_owned()),
                ..Default::default()
            },
        }
    }

    use crate::credential::aggregate::Credential;

    async fn test_state() -> IssuanceState {
        let state = issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;
        initialize(&state).await.unwrap();
        public_command_handler(
            SERVER_CONFIG_ID,
            &state.command.server_config,
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration: serde_json::from_value(json!({
                    "credential_configuration_id": CONFIGURATION,
                    "format": "dc+sd-jwt",
                    "refreshService": {"type": "VerifiableCredentialRefreshService2021"}
                }))
                .unwrap(),
                provisioned: false,
            },
        )
        .await
        .unwrap();
        state
    }

    async fn test_library_state() -> LibraryState {
        let state = agent_library::state::library_state(&InMemory, &Default::default(), Default::default()).await;
        public_command_handler(
            CONFIGURATION,
            &state.command.template,
            TemplateCommand::CreateNewTemplate {
                template_id: CONFIGURATION.to_owned(),
                source_template_id: None,
                title: CONFIGURATION.to_owned(),
                display: Box::new(None),
                data_model: Default::default(),
                holder_type: Default::default(),
                tags: None,
                status: TemplateStatus::Published,
                visibility: Default::default(),
                credential_expiration: Some(Expiration::Never),
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
                holder_authorization: Default::default(),
                origin: None,
            },
        )
        .await
        .unwrap();
        state
    }

    /// Issues the original credential and returns the token it embeds.
    async fn issue_original(state: &IssuanceState) -> RefreshToken {
        let credential_configuration = public_query_handler(SERVER_CONFIG_ID, &state.query.server_config)
            .await
            .unwrap()
            .unwrap()
            .credential_configurations[CONFIGURATION]
            .1
            .clone();
        create_credential(
            state,
            Caller::Internal,
            ORIGINAL,
            CONFIGURATION,
            CredentialCommand::CreateUnsignedCredential {
                credential_id: ORIGINAL.to_owned(),
                data: Data {
                    raw: json!({"first_name": "Ferris", "last_name": "Rustacean"}),
                },
                credential_configuration: Box::new(credential_configuration),
                expires_at: CredentialExpiry::Never,
                refresh_token: None,
            },
        )
        .await
        .unwrap();
        deliver(state, ORIGINAL).await;
        embedded_token(state, ORIGINAL).await
    }

    async fn embedded_token(state: &IssuanceState, credential_id: &str) -> RefreshToken {
        let data = credential(state, credential_id).await.unwrap().data.unwrap();
        RefreshToken::from(data.raw["refreshService"]["refreshToken"].as_str().unwrap().to_owned())
    }

    /// The wallet fetches the credential.
    async fn deliver(state: &IssuanceState, credential_id: &str) {
        public_command_handler(
            credential_id,
            &state.command.credential,
            CredentialCommand::SignCredential {
                credential_id: credential_id.to_owned(),
                subject_id: None,
                overwrite: false,
                proofs: None,
                status_list_id: "status-list".to_owned(),
                index: 0,
            },
        )
        .await
        .unwrap();
    }

    /// The wallet reports `credential_accepted`.
    async fn accept(state: &IssuanceState, credential_id: &str) {
        let notification_id = credential(state, credential_id).await.unwrap().notification_id.unwrap();
        public_command_handler(
            credential_id,
            &state.command.credential,
            CredentialCommand::AddNotification {
                credential_id: credential_id.to_owned(),
                notification: NotificationRequest {
                    notification_id,
                    event: NotificationEvent::CredentialAccepted,
                    event_description: None,
                },
            },
        )
        .await
        .unwrap();
    }

    async fn capability(state: &IssuanceState, token: &RefreshToken) -> RefreshCapability {
        public_query_handler(&token.capability_id(), &state.query.refresh_capability)
            .await
            .unwrap()
            .unwrap()
    }

    /// An issuer with an issued, refreshable original and a preparation hook, and the original's token.
    struct RefreshableOriginal {
        state: IssuanceState,
        library_state: LibraryState,
        hook: Arc<RefreshEverything>,
        token: RefreshToken,
    }

    async fn refreshable() -> RefreshableOriginal {
        let mut state = test_state().await;
        let hook = Arc::new(RefreshEverything::default());
        state.ports.refresh_preparation = hook.clone();
        let token = issue_original(&state).await;
        RefreshableOriginal {
            state,
            library_state: test_library_state().await,
            hook,
            token,
        }
    }

    #[tokio::test]
    async fn an_unknown_token_is_not_found() {
        let RefreshableOriginal {
            state, library_state, ..
        } = refreshable().await;

        let result = refresh_credential(&state, &library_state, &RefreshToken::from("unknown".to_owned())).await;

        assert_eq!(result, Err(RefreshError::NotFound));
    }

    #[tokio::test]
    async fn a_retired_token_is_not_found() {
        let RefreshableOriginal {
            state,
            library_state,
            token,
            ..
        } = refreshable().await;
        public_command_handler(
            &token.capability_id(),
            &state.command.refresh_capability,
            RefreshCapabilityCommand::RetireRefreshCapability {
                accepted_credential_id: "replacement".to_owned(),
            },
        )
        .await
        .unwrap();

        let result = refresh_credential(&state, &library_state, &token).await;

        assert_eq!(result, Err(RefreshError::NotFound));
    }

    #[tokio::test]
    async fn the_default_hook_refuses_refresh() {
        let state = test_state().await;
        let token = issue_original(&state).await;

        let result = refresh_credential(&state, &test_library_state().await, &token).await;

        assert_eq!(result, Err(RefreshError::Unavailable));
    }

    struct FailingHook;

    #[async_trait::async_trait]
    impl RefreshPreparation for FailingHook {
        async fn prepare(&self, _: &Credential, _: &str) -> Result<Replacement, RefreshPreparationError> {
            Err(RefreshPreparationError::Failed(anyhow::anyhow!(
                "the source of truth is down"
            )))
        }
    }

    #[tokio::test]
    async fn a_hook_failure_is_a_preparation_failure() {
        let RefreshableOriginal {
            mut state,
            library_state,
            token,
            ..
        } = refreshable().await;
        state.ports.refresh_preparation = Arc::new(FailingHook);

        let result = refresh_credential(&state, &library_state, &token).await;

        assert_eq!(result, Err(RefreshError::PreparationFailed));
    }

    #[tokio::test]
    async fn a_repeat_call_returns_the_undelivered_pending_offer_without_asking_the_hook() {
        let RefreshableOriginal {
            state,
            library_state,
            hook,
            token,
        } = refreshable().await;
        refresh_credential(&state, &library_state, &token).await.unwrap();

        refresh_credential(&state, &library_state, &token).await.unwrap();

        assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
        assert_eq!(capability(&state, &token).await.continuations.len(), 1);
    }

    /// Holds every preparation until two refreshes are preparing at once.
    struct Overlapping(tokio::sync::Barrier);

    #[async_trait::async_trait]
    impl RefreshPreparation for Overlapping {
        async fn prepare(&self, _: &Credential, _: &str) -> Result<Replacement, RefreshPreparationError> {
            self.0.wait().await;
            Ok(refreshed())
        }
    }

    #[tokio::test]
    async fn concurrent_calls_with_one_token_get_one_offer() {
        let RefreshableOriginal {
            mut state,
            library_state,
            token,
            ..
        } = refreshable().await;
        state.ports.refresh_preparation = Arc::new(Overlapping(tokio::sync::Barrier::new(2)));

        let (first, second) = tokio::join!(
            refresh_credential(&state, &library_state, &token),
            refresh_credential(&state, &library_state, &token)
        );

        assert!(first.is_ok() && second.is_ok());
        assert_eq!(capability(&state, &token).await.continuations.len(), 1);
    }

    #[tokio::test]
    async fn a_delivered_but_unaccepted_offer_gets_a_new_continuation() {
        let RefreshableOriginal {
            state,
            library_state,
            token,
            ..
        } = refreshable().await;
        refresh_credential(&state, &library_state, &token).await.unwrap();
        let delivered = capability(&state, &token).await.continuations[0].credential_id.clone();
        deliver(&state, &delivered).await;

        refresh_credential(&state, &library_state, &token).await.unwrap();

        assert_eq!(capability(&state, &token).await.continuations.len(), 2);
    }

    #[tokio::test]
    async fn an_accepted_replacement_retires_the_token_on_its_next_use() {
        let RefreshableOriginal {
            state,
            library_state,
            token,
            ..
        } = refreshable().await;
        refresh_credential(&state, &library_state, &token).await.unwrap();
        let replacement = capability(&state, &token).await.continuations[0].credential_id.clone();
        deliver(&state, &replacement).await;
        accept(&state, &replacement).await;

        let result = refresh_credential(&state, &library_state, &token).await;

        assert_eq!(result, Err(RefreshError::NotFound));
        assert!(capability(&state, &token).await.retired);
    }

    /// Refuses the first offer a refresh creates, and allows everything else.
    #[derive(Default)]
    struct RefuseFirstOffer(AtomicBool);

    #[async_trait::async_trait]
    impl AuthorizationChecker for RefuseFirstOffer {
        async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            match request.operation {
                AuthorizationOperation::Command {
                    operation_name: "issuance.offers.create",
                    ..
                } if !self.0.swap(true, Ordering::SeqCst) => Err(AuthorizationError::Forbidden),
                _ => Ok(()),
            }
        }
    }

    #[tokio::test]
    async fn a_failed_write_records_no_pending_offer_and_a_retry_starts_clean() {
        let RefreshableOriginal {
            mut state,
            library_state,
            token,
            ..
        } = refreshable().await;
        state.authorization_checker = Arc::new(RefuseFirstOffer::default());
        let failed = refresh_credential(&state, &library_state, &token).await;
        let continuations_after_failure = capability(&state, &token).await.continuations.len();

        let retried = refresh_credential(&state, &library_state, &token).await;

        assert_eq!((failed, continuations_after_failure), (Err(RefreshError::Failed), 0));
        assert!(retried.is_ok());
        assert_eq!(capability(&state, &token).await.continuations.len(), 1);
    }

    #[tokio::test]
    async fn a_refreshable_replacement_gets_its_own_capability() {
        let RefreshableOriginal {
            state,
            library_state,
            token,
            ..
        } = refreshable().await;
        refresh_credential(&state, &library_state, &token).await.unwrap();
        let replacement = capability(&state, &token).await.continuations[0].credential_id.clone();

        let replacement_token = embedded_token(&state, &replacement).await;

        assert_eq!(capability(&state, &replacement_token).await.credential_id, replacement);
    }

    #[tokio::test]
    async fn refresh_leaves_the_original_credential_and_its_status_unchanged() {
        let RefreshableOriginal {
            state,
            library_state,
            token,
            ..
        } = refreshable().await;
        let before = credential(&state, ORIGINAL).await.unwrap();

        refresh_credential(&state, &library_state, &token).await.unwrap();

        assert_eq!(credential(&state, ORIGINAL).await.unwrap(), before);
    }
}

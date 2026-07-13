use agent_authorization::services::OpenId4VpPresentationService;
use agent_shared::{
    generate_random_string,
    handlers::{command_handler, query_handler},
};
use agent_verification::{
    authorization_request::command::AuthorizationRequestCommand, generic_oid4vc::GenericAuthorizationResponse,
    state::VerificationState,
};
use async_trait::async_trait;
use oid4vp::dcql::dcql_query::{ClaimQuery, CredentialQuery, CredentialQueryId, DcqlQuery, Format, MetaTypes};
use shared_kernel::authorization::Caller;
use std::sync::Arc;

/// This adapter bridges `agent_verification` functionality which is needed in `agent_authorization` during the interactive authorization flow, specifically for handling openID4VP presentation requests and responses.
pub struct VerificationAuthorizationAdapter {
    verification_state: Arc<VerificationState>,
}

impl VerificationAuthorizationAdapter {
    #[must_use]
    pub fn new(verification_state: Arc<VerificationState>) -> Self {
        Self { verification_state }
    }
}

#[async_trait]
impl OpenId4VpPresentationService for VerificationAuthorizationAdapter {
    async fn create_openid4vp_presentation_request(&self, state: String) -> anyhow::Result<serde_json::Value> {
        let nonce = generate_random_string();

        // TODO: Make claims and credential formats configurable by the client, rather than hardcoded.
        // Currently only applicable when `enable_interactive_authorization_flow` is true. When false,
        // the authorization server defaults to the Authorization Code flow.
        // The hardcoded claims below are set to request the EduID and Entitlement credentials.
        let eduid_claims: Vec<ClaimQuery> = serde_json::from_value(serde_json::json!([
            {"path": ["name"]},
            {"path": ["given_name"]},
            {"path": ["family_name"]},
            {"path": ["email"]},
            {"path": ["eduperson_scoped_affiliation"]},
            {"path": ["eduperson_assurance"]},
            {"path": ["is_student"]},
            {"path": ["is_faculty"]},
            {"path": ["is_member"]},
            {"path": ["is_staff"]},
            {"path": ["is_alum"]},
            {"path": ["is_affiliate"]},
            {"path": ["is_employee"]},
            {"path": ["is_library-walk-in"]}
        ]))?;

        let entitlement_claims: Vec<ClaimQuery> = serde_json::from_value(serde_json::json!([
            {"path": ["entitlement"]},
        ]))?;

        let dcql_query = DcqlQuery {
            credentials: vec![
                CredentialQuery {
                    id: CredentialQueryId::try_new("eduID").unwrap(),
                    format: Format::DcSdJwt,
                    multiple: None,
                    meta: MetaTypes::SdJwtMeta {
                        vct_values: vec!["https://issuer.pilots.eduid.nl/vct/eduid".to_string()],
                    },
                    trusted_authorities: None,
                    require_cryptographic_holder_binding: None,
                    claims: Some(eduid_claims),
                    claim_sets: None,
                },
                CredentialQuery {
                    id: CredentialQueryId::try_new("Entitlement").unwrap(),
                    format: Format::DcSdJwt,
                    multiple: None,
                    meta: MetaTypes::SdJwtMeta {
                        vct_values: vec!["https://issuer.pilots.eduid.nl/vct/entitlement".to_string()],
                    },
                    trusted_authorities: None,
                    require_cryptographic_holder_binding: None,
                    claims: Some(entitlement_claims),
                    claim_sets: None,
                },
            ],
            credential_sets: None,
        };

        let authorization_request_id = state.clone();

        let command = AuthorizationRequestCommand::CreateAuthorizationRequest {
            state,
            nonce,
            dcql_query: Some(dcql_query),
            alternative_response_mode: None,
        };

        command_handler(
            self.verification_state.authorization_checker.clone(),
            Caller::Internal,
            &authorization_request_id,
            &self.verification_state.command.authorization_request,
            command,
        )
        .await?;

        let mut authorization_request = query_handler(
            self.verification_state.authorization_checker.clone(),
            Caller::Internal,
            &authorization_request_id,
            &self.verification_state.query.authorization_request,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("Authorization request not found"))?
        .authorization_request
        .ok_or_else(|| anyhow::anyhow!("Authorization request is missing"))?
        .as_oid4vp_authorization_request()
        .ok_or_else(|| anyhow::anyhow!("Failed to convert to OID4VP authorization request"))?
        .clone();

        // Normal OID4VP flows would use "direct_post" response mode, which this value is by default, but since this is an interactive authorization flow the "iae_post" response mode must be used. Spec: https://openid.github.io/OpenID4VCI/openid-4-verifiable-credential-issuance-1_1-wg-draft.html#section-6.2.1.1
        authorization_request.body.extension.response_mode = "iae_post".to_string();

        Ok(serde_json::json!(authorization_request))
    }

    async fn verify_openid4vp_response(&self, response: serde_json::Value) -> anyhow::Result<()> {
        let authorization_response: GenericAuthorizationResponse = serde_json::from_value(response)?;

        let authorization_request_id = if let Some(state) = authorization_response.state() {
            state.clone()
        } else {
            return Err(anyhow::anyhow!("Authorization response is missing `state` parameter"));
        };

        // TODO: perhaps this is a good place to invalidate the `state` parameter after it's used, to prevent replay attacks? See comment under `SubmitOpenId4VpResponse` in `agent_authorization/src/domain/oauth2_authorization_request/aggregate.rs` as well.

        let command = AuthorizationRequestCommand::VerifyAuthorizationResponse { authorization_response };

        command_handler(
            self.verification_state.authorization_checker.clone(),
            Caller::Internal,
            &authorization_request_id,
            &self.verification_state.command.authorization_request,
            command,
        )
        .await?;

        Ok(())
    }
}

use agent_authorization::services::OpenId4VpPresentationService;
use agent_shared::handlers::{command_handler, query_handler};
use agent_verification::{
    authorization_request::command::AuthorizationRequestCommand, generic_oid4vc::GenericAuthorizationResponse,
    state::VerificationState,
};
use async_trait::async_trait;
use oid4vp::dcql::dcql_query::{ClaimQuery, CredentialQuery, CredentialQueryId, DcqlQuery, Format, MetaTypes};
use std::sync::Arc;

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
        let nonce = "nonce".to_string();

        let claims: Vec<ClaimQuery> = serde_json::from_value(serde_json::json!([
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

        let dcql_query = DcqlQuery {
            credentials: vec![CredentialQuery {
                id: CredentialQueryId::try_new("eduID").unwrap(),
                format: Format::DcSdJwt,
                multiple: None,
                meta: MetaTypes::SdJwtMeta {
                    vct_values: vec!["https://issuer.pilots.eduid.nl/vct/eduid".to_string()],
                },
                trusted_authorities: None,
                require_cryptographic_holder_binding: None,
                claims: Some(claims),
                claim_sets: None,
            }],
            credential_sets: None,
        };

        let authorization_request_id = state.clone();

        let command = AuthorizationRequestCommand::CreateAuthorizationRequest {
            state,
            nonce,
            dcql_query: Some(dcql_query),
        };

        command_handler(
            &authorization_request_id,
            &self.verification_state.command.authorization_request,
            command,
        )
        .await?;

        let mut authorization_request = query_handler(
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

        // FIXME
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

        let command = AuthorizationRequestCommand::VerifyAuthorizationResponse { authorization_response };

        command_handler(
            &authorization_request_id,
            &self.verification_state.command.authorization_request,
            command,
        )
        .await?;

        Ok(())
    }
}

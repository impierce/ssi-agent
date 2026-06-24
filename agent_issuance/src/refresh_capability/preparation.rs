use async_trait::async_trait;
use thiserror::Error;

use crate::credential::aggregate::CredentialExpiry;

#[derive(Debug, Clone, PartialEq)]
pub struct RefreshPreparationRequest {
    pub refresh_reference: String,
    pub credential_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefreshPreparationInput {
    pub credential_configuration_id: String,
    pub credential: serde_json::Value,
    pub expires_at: CredentialExpiry,
    pub metadata: RefreshPreparationMetadata,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RefreshPreparationMetadata {
    pub reason: Option<String>,
    pub trigger_type: Option<String>,
    pub triggered_by: Option<String>,
}

#[derive(Debug, Error, PartialEq)]
pub enum RefreshPreparationError {
    #[error("Refresh cannot proceed")]
    RefreshUnavailable,
    #[error("Refresh preparation failed: {0}")]
    PreparationFailed(String),
}

#[async_trait]
pub trait RefreshPreparationHook: Send + Sync {
    async fn prepare(
        &self,
        request: &RefreshPreparationRequest,
    ) -> Result<RefreshPreparationInput, RefreshPreparationError>;
}

#[derive(Debug, Default, Clone)]
pub struct NoOpRefreshPreparationHook;

#[async_trait]
impl RefreshPreparationHook for NoOpRefreshPreparationHook {
    async fn prepare(
        &self,
        _request: &RefreshPreparationRequest,
    ) -> Result<RefreshPreparationInput, RefreshPreparationError> {
        Err(RefreshPreparationError::RefreshUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRefreshPreparationHook;

    #[async_trait]
    impl RefreshPreparationHook for TestRefreshPreparationHook {
        async fn prepare(
            &self,
            request: &RefreshPreparationRequest,
        ) -> Result<RefreshPreparationInput, RefreshPreparationError> {
            Ok(RefreshPreparationInput {
                credential_configuration_id: "credential-configuration-id".to_string(),
                credential: serde_json::json!({
                    "subject": request.credential_id
                }),
                expires_at: CredentialExpiry::Never,
                metadata: RefreshPreparationMetadata {
                    reason: Some("test".to_string()),
                    trigger_type: Some("refresh_service".to_string()),
                    triggered_by: None,
                },
            })
        }
    }

    #[async_std::test]
    async fn refresh_preparation_hook_can_return_neutral_preparation_input() {
        let hook = TestRefreshPreparationHook;

        let input = hook
            .prepare(&RefreshPreparationRequest {
                refresh_reference: "refresh-reference".to_string(),
                credential_id: "credential-id".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(input.credential_configuration_id, "credential-configuration-id");
        assert_eq!(input.credential["subject"], "credential-id");
        assert_eq!(input.expires_at, CredentialExpiry::Never);
        assert_eq!(input.metadata.reason.as_deref(), Some("test"));
    }
}

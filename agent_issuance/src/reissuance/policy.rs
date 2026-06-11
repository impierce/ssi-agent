use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReissuancePolicyRequest {
    pub original_credential_id: String,
    pub credential_configuration_id: String,
    pub triggered_by: Option<String>,
    pub trigger_type: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReissuancePolicyError {
    #[error("Reissuance is not allowed")]
    NotAllowed,
}

#[async_trait]
pub trait ReissuancePolicy: Send + Sync {
    async fn authorize(&self, request: &ReissuancePolicyRequest) -> Result<(), ReissuancePolicyError>;
}

#[derive(Debug, Default)]
pub struct NoOpReissuancePolicy;

#[async_trait]
impl ReissuancePolicy for NoOpReissuancePolicy {
    async fn authorize(&self, _request: &ReissuancePolicyRequest) -> Result<(), ReissuancePolicyError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[async_std::test]
    async fn noop_reissuance_policy_allows_requests() {
        let policy = NoOpReissuancePolicy;

        let request = ReissuancePolicyRequest {
            original_credential_id: "original-credential-id".to_string(),
            credential_configuration_id: "credential-configuration-id".to_string(),
            triggered_by: Some("unitrust".to_string()),
            trigger_type: Some("manual".to_string()),
        };

        assert_eq!(policy.authorize(&request).await, Ok(()));
    }
}

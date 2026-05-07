use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationOperation {
    Command {
        aggregate_id: String,
        command_type: &'static str,
    },
    Query {
        query_type: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub actor: Option<Actor>,
    pub operation: AuthorizationOperation,
}

#[async_trait]
pub trait AuthorizationChecker: Send + Sync {
    async fn is_authorized(&self, request: &AuthorizationRequest) -> bool;
}

pub struct AllowAllAuthorizationChecker;

#[async_trait]
impl AuthorizationChecker for AllowAllAuthorizationChecker {
    async fn is_authorized(&self, _request: &AuthorizationRequest) -> bool {
        true
    }
}

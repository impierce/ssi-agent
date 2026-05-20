use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub subject: String,
}

pub trait ToActor {
    fn to_actor(&self) -> Option<Actor>;

    fn auth_value(&self, _key: &str) -> Option<&str> {
        None
    }
}

pub trait ActorExtractor: Send + Sync + 'static {
    fn extract_actor(&self, input: &dyn ToActor) -> Option<Actor>;
}

#[derive(Clone)]
pub struct NoActorExtractor;

impl ActorExtractor for NoActorExtractor {
    fn extract_actor(&self, _input: &dyn ToActor) -> Option<Actor> {
        None
    }
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

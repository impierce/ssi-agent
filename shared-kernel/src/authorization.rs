use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub subject: String,
}

pub trait ToActor {
    fn to_actor(&self) -> Option<Actor>;
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

#[cfg(test)]
mod tests {
    use super::*;

    struct TestActorInput;

    impl ToActor for TestActorInput {
        fn to_actor(&self) -> Option<Actor> {
            Some(Actor {
                subject: "user@example.test".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn allow_all_authorization_checker_authorizes_requests() {
        let checker = AllowAllAuthorizationChecker;
        let request = AuthorizationRequest {
            actor: Some(Actor {
                subject: "user@example.test".to_string(),
            }),
            operation: AuthorizationOperation::Command {
                aggregate_id: "aggregate-id".to_string(),
                command_type: "test-command",
            },
        };

        assert!(checker.is_authorized(&request).await);
    }

    #[test]
    fn no_actor_extractor_does_not_extract_actor() {
        let extractor = NoActorExtractor;

        assert_eq!(extractor.extract_actor(&TestActorInput), None);
    }
}

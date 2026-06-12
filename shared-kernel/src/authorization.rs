use async_trait::async_trait;

/// Identifies the caller on whose behalf an application operation is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    /// Stable subject identifier for the caller.
    pub subject: String,
}

/// Adapter trait for request-like inputs that can expose actor information.
pub trait ToActor: Sync {
    /// Returns the actor represented by this input, if one can be derived.
    fn to_actor(&self) -> Option<Actor>;

    /// Returns an authentication-related value by key when the input can expose one.
    fn auth_value(&self, _key: &str) -> Option<&str> {
        None
    }
}

/// Extracts an [`Actor`] from an input object.
#[async_trait]
pub trait ActorExtractor: Send + Sync + 'static {
    /// Returns the actor that should be attached to the application operation.
    async fn extract_actor(&self, input: &(dyn ToActor + Sync)) -> Option<Actor>;
}

/// Actor extractor used when no actor context should be attached.
#[derive(Clone)]
pub struct NoActorExtractor;

#[async_trait]
impl ActorExtractor for NoActorExtractor {
    async fn extract_actor(&self, _input: &(dyn ToActor + Sync)) -> Option<Actor> {
        None
    }
}

/// Describes the application operation that is being authorized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAuthorization {
    ActiveUser,
    Administrator,
    AdministratorOrEditor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationOperation {
    /// A write-side command against an aggregate.
    Command {
        /// Aggregate instance targeted by the command.
        ///
        /// This lets authorization checkers make instance-scoped decisions, such as allowing an
        /// actor to execute a command type only for a specific credential, offer, or profile.
        aggregate_id: String,
        /// Command type name used as the operation identifier.
        command_type: &'static str,
        authorization: CommandAuthorization,
    },
    /// A read-side query.
    Query {
        /// Query type name used as the operation identifier.
        query_type: &'static str,
    },
}

/// Complete authorization input for an application command or query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    /// Caller context for the operation, if available.
    pub actor: Option<Actor>,
    /// Operation being authorized.
    pub operation: AuthorizationOperation,
}

/// Errors related to authorization checks
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizationError {
    #[error("Unauthorized")]
    Unauthorized, // 401
    #[error("Forbidden")]
    Forbidden, // 403
}

/// Decides whether an [`AuthorizationRequest`] is allowed to execute.
#[async_trait]
pub trait AuthorizationChecker: Send + Sync {
    /// Returns `true` when the request is authorized.
    async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError>;
}

/// Authorization checker that permits every request.
pub struct AllowAllAuthorizationChecker;

#[async_trait]
impl AuthorizationChecker for AllowAllAuthorizationChecker {
    async fn is_authorized(&self, _request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
        Ok(())
    }
}

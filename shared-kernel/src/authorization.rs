use async_trait::async_trait;

/// Identifies the provenance on whose behalf an operation is dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// An external caller for whom no authenticated actor was established.
    Anonymous,

    /// An authenticated external actor.
    Actor(Actor),

    /// Trusted application code dispatching on its own behalf.
    ///
    /// This provenance is intended for fixed continuations of an operation that has already been
    /// authorized, or for application work for which no external actor exists. It is not an
    /// unconditional bypass: authorization checkers decide explicitly which operations permit an
    /// internal caller. Initiating-actor audit lineage is separate from this authorization fact.
    Internal,
}

/// Identifies an authenticated external actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    /// Stable subject identifier for the caller.
    pub subject: String,
}

/// Provides the stable operation name for a command.
///
/// Operation names are compatibility-sensitive authorization identifiers. They describe the
/// attempted operation rather than a permission and must not be renamed casually once published.
pub trait CommandOperation {
    fn operation_name(&self) -> &'static str;
}

/// Provides a stable operation name for an authorized direct view lookup.
///
/// Operation names are compatibility-sensitive authorization identifiers. They describe the
/// attempted operation rather than a permission and must not be renamed casually once published.
///
/// TODO: Replace this view-based contract with typed query values when the direct repository
/// query API is redesigned, or when multiple authorized operations need to return the same view
/// type. The current handler receives no query value, so the view type is temporarily the only
/// compile-time place to associate a stable operation identity. This assumes one operation per
/// view; resource identity is supplied separately at dispatch and remains untyped.
pub trait QueryOperation {
    const OPERATION_NAME: &'static str;
}

/// Adapter trait for request-like inputs that can expose actor information.
pub trait ToActor: Sync {
    /// Returns the actor represented by this input, if one can be derived.
    fn to_actor(&self) -> Option<Actor>;

    /// Returns an authentication-related value by key when the input can expose one.
    fn auth_value(&self, _key: &str) -> Option<&str> {
        None
    }

    /// Returns a bearer token from the `Authorization` value without treating it as an actor.
    fn bearer_token(&self) -> Option<&str> {
        self.auth_value("authorization")
            .and_then(|authorization_header| authorization_header.strip_prefix("Bearer "))
            .filter(|token| !token.is_empty())
    }
}

/// Extracts an [`Actor`] from an input object.
#[async_trait]
pub trait ActorExtractor: Send + Sync + 'static {
    /// Returns the authenticated actor represented by the input, if one can be established.
    async fn extract_actor(&self, input: &dyn ToActor) -> Option<Actor>;
}

/// Actor extractor used when inputs cannot establish an authenticated actor.
#[derive(Clone)]
pub struct NoActorExtractor;

#[async_trait]
impl ActorExtractor for NoActorExtractor {
    async fn extract_actor(&self, _input: &dyn ToActor) -> Option<Actor> {
        None
    }
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

        /// Product resource targeted by the command, when distinct or derivable.
        ///
        /// The shared kernel treats this as an opaque identifier.
        resource_id: Option<String>,

        /// Stable operation identifier chosen by the application context.
        ///
        /// This is intentionally not a permission, role, scope, or attribute. The shared
        /// kernel only reports what operation is being attempted; product code decides how that
        /// operation maps to authorization policy.
        operation_name: &'static str,
    },
    /// A read-side query.
    Query {
        /// Product resource targeted by the query, when one exists.
        ///
        /// List and aggregate queries commonly have no resource identity.
        resource_id: Option<String>,

        /// Stable operation identifier chosen by the application context.
        ///
        /// This gives authorization checkers a neutral key for read policies without requiring
        /// the shared kernel to know product-specific permissions or access attributes.
        operation_name: &'static str,
    },
}

/// Complete authorization input for an application command or query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    /// Provenance of the operation's caller.
    pub caller: Caller,
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
    /// Returns `Ok(())` when the request is authorized.
    async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError>;
}

/// Error returned when an authorization checker is not configured for an application context.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Invalid authorization configuration: {message}")]
pub struct AuthorizationConfigurationError {
    message: String,
}

impl AuthorizationConfigurationError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// An authorization checker whose configuration can be validated for `Context`.
pub trait AuthorizationCheckerFor<Context>: AuthorizationChecker {
    /// Validates that the checker is completely configured for `Context`.
    ///
    /// # Errors
    ///
    /// Returns an error when required authorization configuration is missing or inconsistent.
    fn validate_context(&self) -> Result<(), AuthorizationConfigurationError>;
}

/// Authorization checker that permits every request.
pub struct AllowAllAuthorizationChecker;

#[async_trait]
impl AuthorizationChecker for AllowAllAuthorizationChecker {
    async fn is_authorized(&self, _request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
        Ok(())
    }
}

impl<Context> AuthorizationCheckerFor<Context> for AllowAllAuthorizationChecker {
    fn validate_context(&self) -> Result<(), AuthorizationConfigurationError> {
        Ok(())
    }
}

# Authorization

UniCore's authorization layer determines whether a request is allowed to perform an operation. The following errors may be encountered during authorization checks:

## Forbidden

This error is raised when the request is authenticated or otherwise identified, but the actor is not authorized to perform the requested operation. The system returns a `403 Forbidden` error.

### Resolution

Authenticate or provide sufficient permissions for the requested management operation.

Authorization checks can include both the requested operation and the target aggregate instance. This allows permission
models to be broad, such as allowing a command type in general, or scoped to a specific credential, offer, profile, or
other aggregate instance.

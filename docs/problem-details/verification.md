# Verification

UniCore’s verification process is responsible for generating Authorization Requests and validating received Verifiable
Presentations. This involves constructing and signing the Authorization Requests. The errors documented in this section relate to possible failure scenarios encountered during the verification process, such as missing or invalid Authorization Requests, errors in signing the Authorization Request, or issues with the Authorization Request Builder.

## Authorization Request Builder Error

This error indicates that UniCore failed to construct an Authorization Request. This error should never occur and when
it does, this indicates a bug in UniCore.

### Resolution

This error is not caused by incorrect client input but reflects a flaw in the internal processing of the Authorization Request. If you encounter this error, please report it to the development team along with any relevant logs and context, so that the issue can be investigated and resolved.

## Missing Authorization Request

This error is raised when UniCore attempts to sign an Authorization Request but finds that none exists. In other words,
the expected Authorization Request was not created or persisted before the signing process was initiated.

### Resolution

This error is not caused by incorrect client input but reflects a flaw in the internal processing of the Authorization Request. If you encounter this error, please report it to the development team along with any relevant logs and context, so that the issue can be investigated and resolved.

## Authorization Request Signing Error

This error is raised when the system fails to sign an Authorization Request.

### Resolution

This error is not caused by incorrect client input but reflects a flaw in the internal processing of the Authorization
Request. If you encounter this error, please report it to the development team along with any relevant logs and context,
so that the issue can be investigated and resolved.

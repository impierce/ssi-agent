## Connection Not Found

This error occurs when a request references a connection_id which does not match existing connections in the system. The specified `connection_id` may have been deleted, never created, or is incorrect.

## Credential Issuer Metadata Fetch Failed

This error occurs when the system is unable to retrieve credential issuer metadata from the connection's `/.well-known/openid-credential-issuer` endpoint. This may be caused by the remote server being unreachable, the URL being incorrect, or the endpoint not returning a valid metadata response.

## Missing Domain

This error should not occur as the domain is required to create a connection.
This error indicates that a connection is missing a required `domain` value. The domain is necessary for establishing and verifying the connection, and without it the operation cannot proceed.

## DID Configurations Could Not Be Resolved

This error occurs when the system fails to resolve or fetch DID Configurations from the Connection's `/.well-known/did-configuration.json` domain endpoint.

## Opaque Origin Not Supported

This error occurs when an origin supplied to `add-linked-domains` or `remove-linked-domains` has no host at all, such as a `data:` URL. A linked domain must identify a public host.

## Host Must Be A Domain Name

This error occurs when an origin supplied to `add-linked-domains` or `remove-linked-domains` is an IP address. Domain linkage requires a domain name, not an IP address.

## Invalid Domain Or Origin

This error occurs when an origin supplied to `add-linked-domains` or `remove-linked-domains` could not be parsed as a bare host (e.g. `example.org`) or a full origin (e.g. `https://example.org`). A single invalid entry rejects the whole request.

## No Origins Given

This error occurs when `add-linked-domains` or `remove-linked-domains` is called with an empty `origins` array. At least one origin is required; there is no request that links or unlinks nothing.

## Connection Not Found

This error occurs when a request references a connection_id which does not match existing connections in the system. The specified `connection_id` may have been deleted, never created, or is incorrect.

## Connection Synchronization Failed

This error indicates that the system was unable to synchronize the latest state of a connection. This could indicate issues reaching the issuer's domain or /.well-known endpoints.

## Credential Issuer Metadata Fetch Failed

This error occurs when the system is unable to retrieve credential issuer metadata from the connection's domain. This may be caused by the remote server being unreachable, the URL being incorrect, or the endpoint not returning a valid metadata response.

## Missing Domain

This error should not occur as the domain is required to create a connection.
This error indicates that a connection is missing a required `domain` value. The domain is necessary for establishing and verifying the connection, and without it the operation cannot proceed.

## DID Configurations Could Not Be Resolved

This error occurs when the system fails to resolve or fetch DID Configurations from the Connection's `/.well-known/did-configuration.json` domain endpoint.

# Request

These errors are returned before a request reaches any endpoint, when the request itself cannot be accepted.

## Unsupported Character

This error occurs when the request URI or body contains a `NUL` character, either as a raw byte, percent-encoded as `%00` or as the JSON escape `\u0000`. UniCore cannot store this character, so the request is rejected. The system returns a `400 Bad Request` error.

### Resolution

Remove the `NUL` character from the request and try again.

## Payload Too Large

This error occurs when the request body exceeds the maximum allowed size. The request is rejected before its body is read in full. The system returns a `413 Payload Too Large` error, and its detail states the limit.

### Resolution

Send a smaller request, for example by splitting a batch of credentials into several batches. If such requests are legitimate for your deployment, raise the limit with [`max_request_body_bytes`](../configuration/CONFIGURATION.md#maximum-request-body-size).

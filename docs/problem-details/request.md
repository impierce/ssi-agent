# Request

These errors are returned before a request reaches any endpoint, when the request itself cannot be accepted.

## Unsupported Character

This error occurs when the request URI or body contains a `NUL` character, either as a raw byte, percent-encoded as `%00` or as the JSON escape `\u0000`. UniCore cannot store this character, so the request is rejected. The system returns a `400 Bad Request` error.

### Resolution

Remove the `NUL` character from the request and try again.

## Invalid Query

The query string does not fit the operation's query parameters: a value has the wrong type or is out of range, e.g. a
`limit` that is not a number. Returned with status `400 Bad Request`; the `detail` names the offending parameter.

### Resolution

Compare the query string with the operation's query parameters in the OpenAPI specification.

## Invalid Path

A path parameter does not fit the operation, e.g. an unknown key provider type. Returned with status `400 Bad Request`;
the `detail` names the offending segment.

### Resolution

Compare the path with the operation's path parameters in the OpenAPI specification.

## Unsupported Media Type

The operation takes a JSON request body, but the request carries no `Content-Type` header or one other than
`application/json`. Returned with status `415 Unsupported Media Type`.

### Resolution

Send the request body as JSON with the header `Content-Type: application/json`.

## Payload Too Large

This error occurs when the request body exceeds the maximum allowed size. The request is rejected before its body is read in full. The system returns a `413 Payload Too Large` error, and its detail states the limit.

### Resolution

Send a smaller request, for example by splitting a batch of credentials into several batches. If such requests are legitimate for your deployment, raise the limit with [`max_request_body_bytes`](../configuration/CONFIGURATION.md#maximum-request-body-size).

## Malformed Body

The request body is not valid JSON. Returned with status `400 Bad Request`; the `detail` names the position of the
syntax error.

### Resolution

Send a well-formed JSON document.

## Invalid Body

The request body is valid JSON, but does not match the request schema of the operation: it is not a JSON object, a
required field is missing, or a field has the wrong type. Returned with status `422 Unprocessable Content`; the
`detail` names the offending field.

### Resolution

Compare the request body with the request schema of the operation in the OpenAPI specification.

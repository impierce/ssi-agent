## Request Errors

These errors are returned before a request reaches the operation it addresses, when the request itself cannot be
read.

## Unsupported Character

The request contains a `NUL` character, in its path, query or body (`\u0000` in JSON). The event stores cannot hold
one, so UniCore rejects such requests with status `400 Bad Request` before they reach any operation.

### Resolution

Remove the `NUL` characters from the request.

## Unsupported Media Type

The operation takes a JSON request body, but the request carries no `Content-Type` header or one other than
`application/json`. Returned with status `415 Unsupported Media Type`.

### Resolution

Send the request body as JSON with the header `Content-Type: application/json`.

## Body Too Large

The request body exceeds the size limit of 2 MB. Returned with status `413 Content Too Large`.

### Resolution

Send a smaller request body, e.g. split a batch into several requests.

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

# Events

UniCore's event streaming subsystem broadcasts domain events conforming to the CNCF CloudEvents specification. The following errors may occur when consuming or subscribing to events:

## Event Bus Lagged

This error occurs when an event consumer or subscriber falls behind the real-time event stream and intermediate messages were evicted from the buffer. The system returns a `503 Service Unavailable` error (or emits an SSE `lagged` event frame).

### Resolution

For Server-Sent Events (SSE) connections, reconnecting with the latest acknowledged `Last-Event-ID` will replay historical events from persistent storage. If the gap cannot be filled (e.g., evicted from retention), clients maintaining cached projections should trigger a full state refresh against the primary REST API endpoints before resuming stream consumption.

## Event Source Error

This error occurs when the underlying event store or streaming adapter (such as MongoDB Change Streams) encounters a failure while querying history or reading live oplog records. The system returns a `500 Internal Server Error`.

### Resolution

Verify that the underlying database cluster is healthy, accessible, and properly configured for change streams (e.g. MongoDB replica set). Check application logs for detailed database driver diagnostics and retry the operation.

## Unsupported Position

This error is returned when an event subscription request provides a resume position or format that is not supported by the configured event store adapter. The system returns a `400 Bad Request` error.

### Resolution

Ensure that the resume token or position identifier was obtained from a valid prior event stream response and has not been corrupted or tampered with.

## Event Bus Closed

This error occurs when the internal broadcast channel or event streaming bus has shut down, usually during application termination or reconfiguration. The system returns a `503 Service Unavailable` error.

### Resolution

Wait for the service to finish restarting and reconnect.

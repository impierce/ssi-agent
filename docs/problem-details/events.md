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

This error is returned when an event subscription request provides a resume position or format that is not supported by the configured event store adapter — a resume token that does not decode, or one issued by a different adapter. The system returns a `400 Bad Request` error.

### Resolution

Ensure that the resume token or position identifier was obtained from a valid prior event stream response and has not been corrupted or tampered with.

## Event Bus Closed

This error occurs when the internal broadcast channel or event streaming bus has shut down, usually during application termination or reconfiguration. The system returns a `503 Service Unavailable` error.

### Resolution

Wait for the service to finish restarting and reconnect.

## Catch-Up Truncated

This is not an error response but an SSE `truncated` event frame, emitted when historical catch-up stopped early because more events matched than could be sent. The connection continues into the live stream, so events between the last one replayed and the moment the subscription opened were not delivered.

The frame carries a `resume_after` cursor, both in its payload and as the frame's own `id`.

### Resolution

Reconnect using `resume_after` as the `Last-Event-ID` to continue from that point, repeating until no `truncated` frame is emitted. A browser `EventSource` does this automatically, since the cursor is the frame's `id`. Clients maintaining cached projections should instead refresh state against the primary REST API endpoints.

## Delivery guarantees

`GET /v0/events` is a **live feed, not a source of truth.**

- Catch-up is bounded by the `limit` parameter, which defaults to 100 and is capped at 1000. A `truncated` frame tells you when more events matched than were sent, and carries a cursor to continue from.
- A subscriber that reconnects can miss events. Where that is detectable the stream says so, with a `lagged` or `truncated` frame, but it is not detectable in every case.
- These guarantees assume the default MongoDB event store. On other event stores the stream is best-effort and considerably more limited, and nothing in the response distinguishes them.

Use it for an activity feed, a notification channel, or anything else that tolerates a missed event. Do not build a projection, a synchronisation process or an audit trail on it — those must reconcile against the REST API endpoints rather than treat this stream as complete.

# Events

UniCore's event streaming subsystem broadcasts domain events conforming to the CNCF CloudEvents specification. The following errors may occur when consuming or subscribing to events:

## Event Bus Lagged

This error occurs when an event consumer or subscriber falls behind the real-time event stream and intermediate messages were evicted from the buffer. The system returns a `503 Service Unavailable` error (or emits an SSE `lagged` event frame).

### Resolution

For Server-Sent Events (SSE) connections, reconnecting with the latest acknowledged `Last-Event-ID` will replay historical events from persistent storage. If the gap cannot be filled (e.g., evicted from retention), clients maintaining cached projections should trigger a full state refresh against the primary REST API endpoints before resuming stream consumption.

## Event Source Error

This error occurs when the underlying event store or streaming adapter (such as MongoDB Change Streams) encounters a failure while querying history or reading live oplog records. The system returns a `500 Internal Server Error`.

### Resolution

Verify that the underlying database cluster is healthy, accessible, and properly configured for change streams (e.g. MongoDB replica set), then retry the operation. The `detail` field is deliberately fixed: driver diagnostics routinely carry hostnames, ports, replica-set topology and authentication detail, so the full error is written to the application logs instead of the response.

## Unsupported Position

This error is returned when an event subscription request provides a resume position or format that is not supported by the configured event store adapter — a resume token that does not decode, or one issued by a different adapter. The system returns a `400 Bad Request` error.

On MongoDB this surfaces internally rather than to a client: the change-stream listener reports it, retries, and falls back to a live subscription after three consecutive failures, which skips the events in the gap.

### Resolution

Ensure that the resume token or position identifier was obtained from a valid prior event stream response and has not been corrupted or tampered with.

## Event Bus Closed

This error occurs when the internal broadcast channel or event streaming bus has shut down, usually during application termination or reconfiguration. The system returns a `503 Service Unavailable` error.

Neither event store that ships today produces it: the internal bus holds its channel open for the lifetime of the service, so it never closes while the service is running. The error stays defined for future event store integrations whose connection to the underlying stream can drop.

### Resolution

Wait for the service to finish restarting and reconnect.

## Catch-Up Truncated

This is not an error response but an SSE `truncated` event frame, emitted after the catch-up phase when historical replay stopped on a limit rather than on exhausting matching events. Because the connection then chains straight to the live stream, events between the last replayed event and the moment the subscription opened are delivered by neither.

### Resolution

Reconnect with the `Last-Event-ID` of the final replayed event to continue catch-up from that point, repeating until no `truncated` frame is emitted. Clients maintaining cached projections should instead refresh state against the primary REST API endpoints.

## Delivery guarantees

`GET /v0/events` is a **live feed, not a source of truth.** Three limits apply:

- Catch-up is bounded by the `limit` query parameter, which defaults to 100 and is clamped to 1000.
- On MongoDB, `types`, `since` and `until` are applied after reading rather than in the query, so the scan is additionally bounded by a ceiling on documents examined. A highly selective filter over a large collection can therefore return fewer events than exist.
- Resume ordering uses the event store's `_id`, which reflects the order identifiers were assigned rather than the order transactions committed. Under concurrent writes, or across replicas, a resume can skip an event that committed late with a lower identifier.

Gaps are signalled where they can be detected — `lagged` when `Last-Event-ID` could not be resolved, `truncated` when catch-up stopped on a limit — but they are not prevented, and the ordering case above is not detectable. The stream is appropriate for a UI activity feed. Projections, synchronisation and audit consumers must reconcile against the REST API endpoints rather than treat this stream as complete.

### The guarantees above assume a MongoDB event store

They are weaker on any other backend, and nothing in the response distinguishes the two.

MongoDB is the default and the configuration this endpoint was built against. Only that backend registers a persistent history reader and a change-stream listener, so only there does the endpoint behave as described above. On a PostgreSQL or in-memory event store:

- Catch-up is served from an in-process ring buffer holding the **last 500 events across all aggregates**, not from the event store. Anything older is unavailable regardless of the `limit` requested, and `Last-Event-ID` resolves only within that window.
- The ring buffer is process memory. It is empty after every restart, so a client reconnecting across a deployment gets no history at all.
- The stream carries only events that **this process** dispatched. In a multi-replica deployment, each replica's stream shows only its own writes; on MongoDB the change stream makes every replica's events visible on every replica.

A client cannot tell which backend it is talking to. If you deploy UniCore on PostgreSQL and expose this endpoint, treat it as a best-effort feed of the local process rather than a view of the system.

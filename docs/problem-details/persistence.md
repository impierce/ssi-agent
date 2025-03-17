# Persistence

UniCore's persistence layer is responsible for storing and retrieving the system's state and events. The following errors may be encountered during persistence operations:

## Aggregate Conflict

This error is raised when a command is rejected due to an aggregate conflict, often caused by optimistic locking
failures or concurrent modifications. The system returns a `503 Service Unavailable` error, indicating that the server
is temporarily unable to process the request—possibly due to overload.

### Resolution

Retry the operation after a brief wait. If the problem persists, investigate system load and database performance to address potential concurrency or performance issues.

## Database Connection Error

This error occurs when the system is unable to establish a connection with the database during persistence operations.
It can be caused by network issues, misconfigured database settings, or downtime on the database server.

### Resolution

Verify the database connection settings and ensure that the database server is operational. Consult system logs for additional details to diagnose and resolve connection issues.

## Deserialization Error

This error is raised when the system fails to deserialize events from the event store. Typically, this occurs due to a
schema mismatch—often resulting from changes in the event format without supporting data migration.

### Resolution

Because data migration is currently not supported in UniCore, the only resolution is to reset the event store by wiping the existing data.

## Unexpected Error

This error represents an unforeseen issue during persistence operations that does not fall under the other specific
categories.

### Resolution

Examine the error details and system logs to identify the root cause. This error typically signals a bug or an unhandled
scenario in the persistence logic that should be addressed by the UniCore development team.

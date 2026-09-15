# In-memory projections

MongoDB deployments can set `event_store.views: in_memory` to keep query projections in the
application process and rebuild them from the immutable event stream at startup. This removes the
growing write cost and MongoDB document-size limit of the persisted `all_*` views.

## Topology

In-memory projection mode supports exactly one active UniCore application writer per MongoDB
database. MongoDB may still be a replica set, an Atlas deployment, or a sharded cluster. The
restriction applies to application processes, not database members.

The application enforces this with a renewable MongoDB lease. Event appends validate the lease's
owner, fencing token, and server-time expiry in the same transaction as the append. A second
process remains unready until it can acquire the lease. Lease loss marks the current process
unready and initiates graceful shutdown.

Use one application replica and a rolling deployment with:

```yaml
spec:
  replicas: 1
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 1
```

Configure a startup probe against `/readyz`. Its failure threshold must cover the observed maximum
lease wait plus a full replay of production-sized data. Measure that duration in the deployment;
do not derive it from an empty database.

## Enabling the mode

The first switch from `persisted` to `in_memory` is not a normal rolling handoff because the old
version does not participate in the lease. Drain and stop the old application before starting the
first in-memory-mode instance.

```yaml
event_store:
  type: mongodb
  views: in_memory
  connection_string: mongodb://mongodb:27017/unicore?replicaSet=rs0
```

MongoDB transactions are required for fenced event appends, so the server must be configured as a
replica set or sharded cluster. A standalone MongoDB server is not supported by this mode.

Replay failure aborts startup before identity or issuer initialization. `/livez` and `/healthz`
continue to answer while replay is running, while `/readyz` remains unavailable.

## Data and rollback

The `events` collection is the source of truth and must be preserved. The view collections are
derived state. Once every deployed version uses in-memory projections, the old view collections
may be backed up and dropped. Keep them during the opt-in release if rollback to `persisted` view
storage is required.

Switching back to `persisted` after writes occurred in in-memory mode requires rebuilding or
otherwise catching up the persisted views first; they are not maintained while in-memory mode is
active.

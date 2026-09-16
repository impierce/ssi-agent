# In-memory projections

UniCore keeps query projections in the application process and rebuilds them from the immutable
event stream at startup. This applies to MongoDB and PostgreSQL event stores and removes the
growing write cost and database size limits of persisted `all_*` views.

See [testing and deployment assurance](./in-memory-projections-testing.md) for a staged validation
and Kubernetes rollout procedure.

## Topology

UniCore supports exactly one active application writer per event-store database. MongoDB may still
be a replica set, an Atlas deployment, or a sharded cluster, and PostgreSQL may still use its normal
high-availability topology. The restriction applies to UniCore processes, not database members.

The application enforces this with a database-backed writer lease. Event appends validate a
fencing token in the same transaction as the append. A second process remains unready until it can
acquire the lease. Lease loss marks the current process unready and initiates graceful shutdown.

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

## Event-store configuration

Projection storage is not configurable. `event_store.type` selects where immutable events are
persisted; projections are always rebuilt in memory. For example:

```yaml
event_store:
  type: mongodb
  connection_string: mongodb://mongodb:27017/unicore?replicaSet=rs0
```

MongoDB transactions are required for fenced event appends, so MongoDB must be configured as a
replica set or sharded cluster. A standalone MongoDB server is not supported.

PostgreSQL uses an `application_leases` table for fencing. UniCore creates it at startup when the
database role has schema privileges. Otherwise provision it before deployment using the definition
in `agent_application/docker/db/init.sql`.

When upgrading from a UniCore version that persists views, drain and stop the old application
before starting the new version. The old version does not participate in the writer lease, so the
first transition is not a normal rolling handoff.

Replay failure aborts startup before identity or issuer initialization. `/livez` and `/healthz`
continue to answer while replay is running, while `/readyz` remains unavailable.

## Data and rollback

The `events` collection or table is the source of truth and must be preserved. Legacy view
collections and tables are derived state. They may be retained during the first rollout as a
limited rollback aid and backed up or dropped after the deployment is accepted.

Rolling back to a version that persists views after the new version has accepted writes requires
rebuilding or otherwise catching up those legacy views first. They are no longer maintained.

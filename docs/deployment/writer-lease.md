# Event-store writer lease

One UniCore process at a time appends events to a MongoDB or PostgreSQL event store. The in-memory event store has no
lease.

## Behavior

- **Startup:** the lease is acquired before any event is written. A process waiting for it logs `Waiting for the
  event-store writer lease held by another process` once, and does not serve HTTP until it holds the lease.
- **Appends:** every event append checks the lease in its transaction: owner and fencing token. An append without the
  lease fails, also when the process has not noticed the loss yet.
- **Shutdown:** on `SIGTERM` or `Ctrl+C`, `/readyz` answers `503`, requests in flight get up to 20 seconds to finish
  (open `/v0/events` streams are closed then), then the lease is released.
- **Lease lost** (lease connection broken, renewal failed, taken over by another process): `/readyz` answers `503`,
  the process shuts down like on `SIGTERM` and exits with an error.

### PostgreSQL

- Session-level advisory lock, held on a dedicated connection and checked every 10 seconds; a broken connection
  releases it at once
- Fencing token in the `application_leases` table, created at startup. Without the privilege to create tables, create
  it beforehand as in `agent_application/docker/db/init.sql`.

### MongoDB

- Document in the `application_leases` collection: owner, fencing token, expiry after 30 seconds, renewed every 10
  seconds
- Without a successful renewal for 20 seconds, the holder stops appending
- A released lease is taken over within a second; the lease of a process stopped without releasing it, once it has
  expired (at most 30 seconds)
- Appends run in transactions, which need a replica set or sharded cluster, as before

## Kubernetes

- One replica per event store
- `strategy: Recreate`, or `RollingUpdate` with `maxUnavailable: 1`: with `maxUnavailable: 0`, the new pod waits for
  the lease of the old pod, which is only stopped once the new pod is ready
- `startupProbe` on `/readyz`, with a failure threshold covering the wait for the lease and the startup; the HTTP
  port is not bound while waiting
- `terminationGracePeriodSeconds` above 20 seconds (default: 30), so the lease is released before the pod is killed

# Memory

## Recommended requests and limits

| Resource | Value |
| --- | --- |
| Memory request | `128Mi` |
| Memory limit | `512Mi` |

An idle instance with a MongoDB event store uses about 45 MiB. Under sustained load from 16 concurrent clients
(template changes, credential and offer creation, catalog changes and list requests), memory peaked at 324 MiB and
returned to about 80 MiB within seconds after the load ended. The limit leaves headroom for larger event stores and
longer bursts; see [What memory grows with](#what-memory-grows-with).

## What memory grows with

- **Concurrent commands on long-lived aggregates.** With the MongoDB event store, loading an aggregate replays its events
  one at a time, so a single load needs memory for one event and the aggregate state, not for the whole history. With
  the PostgreSQL event store, a load holds the whole history of the aggregate in memory. The server config is the
  largest aggregate: each of its events carries every credential configuration, so it grows with the number of
  published templates.
- **List views.** Lists such as all credentials or all offers are stored as one document each. Every command that
  changes an item of that list, and every request for the list, loads the whole document, so it grows with the number
  of credentials and offers issued.
- **Startup.** Persisted events are verified at startup while they are read, so verification does not hold the event
  store in memory.

The server config only gets a new event when a credential configuration actually changes: publishing, unpublishing or
deleting a template, or changing the title, display, type, schema, schema attributes or holder authorization of a
published template. Other template edits and restarts do not add events.

To check the size of the server config history and of the list views in MongoDB:

```javascript
db.events.aggregate([
  { $match: { aggregate_type: "server_config" } },
  { $group: { _id: null, events: { $sum: 1 }, bytes: { $sum: { $bsonSize: "$$ROOT" } } } },
]);
["all_credentials", "all_offers", "all_templates"].forEach((name) =>
  printjson({ name, bytes: db.getCollection(name).aggregate([{ $project: { s: { $bsonSize: "$$ROOT" } } }]).toArray() }),
);
```

## Allocator

On Linux and macOS the agent uses [jemalloc](https://jemalloc.net) with a background thread that returns freed memory to
the system after one second, so resident memory falls back after a burst instead of staying at its peak. The defaults
can be overridden with the `_RJEM_MALLOC_CONF` environment variable, e.g. `_RJEM_MALLOC_CONF=dirty_decay_ms:5000`.

## Measurements

Measured with a release build against MongoDB, on a copy of an event store with 2,357 events, 258 of them server config
events (13 MB). Values are resident memory.

| Scenario | Before | After |
| --- | --- | --- |
| Idle after startup | 244 MiB | 45 MiB |
| 200 template edits outside the credential configuration, 4 clients: peak | 1.29 GB | 66 MiB |
| 200 title changes of published templates, 4 clients: peak / 10 s later | 1.63 GB / 1.67 GB | 161 MiB / 53 MiB |
| Mixed load, 16 clients, 3 minutes: peak / 10 s later | 1.53 GB / 1.51 GB | 324 MiB / 81 MiB |
| Mixed load, 16 clients, 3 minutes: requests completed | 6,847 | 12,721 |

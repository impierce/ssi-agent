# Event Store Infrastructure

Implementations of event sourcing and view projection stores for use by bounded contexts.

## Available Stores

- **store-mongodb** — MongoDB-backed event store and view repository

## Usage

Bounded contexts depend on `shared-kernel` factory traits (`CommandHandlerFactory`, `ViewRepositoryFactory`) rather than concrete store implementations. Store implementations provide these traits, enabling simple dependency injection during application startup.

### Example

```rust
use store_mongodb::MongoDBStore;

let store = MongoDBStore::new("mongodb://localhost:27017").await;
let handler = store.create_handler::<MyAggregate>(services, queries).await;
```

## Adding a New Store

1. Create a new crate (`store-your-backend`)
2. Implement `CommandHandlerFactory` and `ViewRepositoryFactory` from `shared-kernel`
3. Wire the store into the application startup

See `store-mongodb` for a reference implementation.

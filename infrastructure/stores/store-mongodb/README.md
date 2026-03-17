# MongoDB Event Store

MongoDB-backed implementation of the shared-kernel persistence factories.

## Overview

Provides `CommandHandlerFactory` and `ViewRepositoryFactory` implementations using:

- **mongo-es** — MongoDB event store and view repository
- **CqrsFramework** — Event sourcing and command handling

## Usage

```rust
use store_mongodb::MongoDBStore;

// Initialize the store (once, during application startup)
let store = MongoDBStore::new("mongodb://localhost:27017").await;

// Create command handlers for aggregates
let handler = store.create_handler::<MyAggregate>(services, queries).await;

// Create view repositories for projections
let repo = store.create_view_repository::<MyView, MyAggregate>("my-view-collection");
```

## Configuration

Configure MongoDB connection string via environment variable or configuration file.

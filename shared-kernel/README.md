# Shared Kernel

Infrastructure and abstractions shared across all bounded contexts in the SSI Agent.

## Overview

The shared kernel provides domain-driven design patterns that every bounded context needs:

- **Command/Query separation** via an actor-style [`ApplicationService`](src/application_service.rs)
- **Command execution abstractions** for decoupling from specific event stores
- **View repository patterns** with dyn-compatible wrappers for different persistence backends
- **Standard CQRS queries** to avoid repetitive boilerplate
- **Type-keyed service registry** for runtime dependency injection

## Architecture

### Application Service

An actor-style service that serialises access to an [`ApplicationContext`], processing commands and queries sequentially over `mpsc` channels. This pattern ensures:

- **No interior mutability** — the context is owned by the service, not wrapped in `Mutex`
- **Concurrent channel senders** — the presentation layer can send messages concurrently
- **Single-threaded context access** — the context itself is never accessed concurrently

See [`application_service`](src/application_service.rs) for the full pattern.

### Command Handler Factory

Bounded contexts use [`CommandHandlerFactory`](src/command_handler.rs) to construct aggregate command handlers without knowing which persistent store is in use:

```rust
let store: Arc<dyn CommandHandlerFactory> = Arc::new(InMemoryStore);
let handler: CommandHandler<MyAggregate> = store.create_handler(services, queries).await;
```

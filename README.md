# SSI Agent

## Software Design

- [Hexagonal architecture](<https://en.wikipedia.org/wiki/Hexagonal_architecture_(software)>)
- [Domain-driven design](https://en.wikipedia.org/wiki/Domain-driven_design)

### Default adapters

| Adapter       | default                  |
| ------------- | ------------------------ |
| `api`         | `REST over HTTP`         |
| `storage`     | `SurrealDB (in-memory)`  |
| `key_manager` | `Stronghold (in-memory)` |
| `messaging`   | _n/a_                    |

## Crates

- `agent_core`: main application logic and data models
- `agent_api`: APIs and servers to run the agent in a web environment
- `agent_storage`: persistence layer for the data the agent produces
- `agent_key_manager`: key management layer for managing signatures and encryption (external keys)
- `agent_messaging`: messaging adapters for incoming and outgoing messages (event streams)

> TODO: individual crate per adapter? `agent_api` --> `agent_api_rest`, `agent_api_graphql`, `agent_api_grpc`, etc.

## Development

To run a dev server, use the following command:

```bash
cd agent_api
cargo run
```

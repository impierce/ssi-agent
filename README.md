# SSI Agent

## Crates

- `agent_core`: main application logic and data models
- `agent_api`: APIs and servers to run the agent in a web environment
- `agent_storage`: persistence layer for the data the agent produces
- `agent_kms`: key management layer for managing signatures and encryption (external keys)
- `agent_messaging`: messaging adapters for incoming and outgoing messages (event streams)

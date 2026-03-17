# Connection

A Connection represents a relationship with an external organisation, tracking its identity (DIDs), display metadata, and domain linkage validation status. It also tracks any pending changes to the organisation's identity, such as changes to their DIDs or display properties through a sync command which refetches the necessary data from the organisation's url.

This aggregate holds everything related to a connection:

- connection_id
- url
- display properties (name, logo, etc.)
- linked_dids
- validation of linked dids
- first_interacted_at
- last_interacted_at
- pending changes

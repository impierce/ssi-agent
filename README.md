# SSI Agent

## API specification

[Follow these instructions](./agent_api_rest/README.md) to inspect the REST API.

## Build & Run

Build and run the **SSI Agent** in a local Docker environment following [these steps](./agent_application/docker/README.md).

```mermaid
sequenceDiagram
    participant wallet as Wallet
    participant client as Client

    box rgb(33,66,99) UniCore
    participant api_rest as API<br/>(agent_api_rest)
    participant issuance as Core Issuance Agent<br/>(agent_issuance)
    participant event_store as Event Store<br/>(agent_store)
    end

    autonumber

    Note over api_rest, event_store: Command and Query<br/>Responsibility Segregation (CQRS) 

    Note over client, issuance: Agent Preparations

    client->>api_rest: POST /v1/credentials<br/>subjectId: <string><br/>credential: <object>
    api_rest->>issuance: Command
    issuance->>event_store: Event(s)
    api_rest->>event_store: Query
    event_store->>api_rest: View
    api_rest->>client: 201 CREATED application/json

    client->>api_rest: POST /v1/offers<br/>subjectId: <string>
    api_rest->>issuance: Command
    issuance->>event_store: Event(s)
    api_rest->>event_store: Query
    event_store->>api_rest: View
    api_rest->>client: 200 OK text/plain

    Note over wallet, api_rest: OpenID4VCI Pre-Authorized<br/>Code Flow

    wallet->>api_rest: GET /.well-known/oauth-authorization-server
    api_rest->>event_store: Query
    event_store->>api_rest: View
    api_rest->>wallet: 200 OK application/json    
    

    wallet->>api_rest: GET /.well-known/openid-credential-issuer
    api_rest->>event_store: Query
    event_store->>api_rest: View
    api_rest->>wallet: 200 OK application/json

    wallet->>api_rest: POST /auth/token
    api_rest->>issuance: Command
    issuance->>event_store: Event(s)
    api_rest->>event_store: Query
    event_store->>api_rest: View
    api_rest->>wallet: 200 OK application/json

    wallet->>api_rest: POST /openid4vci/credential
    api_rest->>issuance: Command
    issuance->>event_store: Event(s)
    api_rest->>event_store: Query
    event_store->>api_rest: View
    api_rest->>wallet: 200 OK application/json


```
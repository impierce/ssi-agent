# UniCore Identity Agent

This module manages decentralized identity (DID) documents and related identity components for UniCore.

## Components

- **Document**: DID document creation, management, and hosting
- **Service**: DID document service endpoints management
- **Connection**: Managing connections to external identity services
- **Profile**: Managing identity profile information and display data

## Features

- DID document creation and management
- Multiple DID method support (did:web, did:iota, etc.)
- Service endpoint configuration
- Domain linkage support
- Profile and display information management
- Integration with UniCore's CQRS event-sourcing architecture

## Supported DID Methods

- `did:web` - Web-based DIDs hosted via HTTPS
- `did:iota` - IOTA-based decentralized identifiers
- Additional methods may be supported based on configuration

This module enables UniCore to manage its decentralized identity, publish DID documents, and interact with other entities in the SSI ecosystem.
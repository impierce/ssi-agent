# UniCore Verification Agent

This module implements credential verification functionality for UniCore, handling the verification of verifiable credentials and presentations.

## Components

- **Authorization Request**: Managing authorization requests for credential verification
- **Generic OID4VC**: Generic OpenID for Verifiable Credentials functionality

## Features

- Verifiable credential verification
- Verifiable presentation verification
- Authorization request processing
- OpenID4VP (OpenID for Verifiable Presentations) support
- Integration with UniCore's CQRS event-sourcing architecture

This module enables UniCore to act as a verifier in SSI ecosystems, validating credentials and presentations according to established standards.

## Standards Supported

- OpenID for Verifiable Presentations (OpenID4VP)
- W3C Verifiable Credentials Data Model
- DID (Decentralized Identifiers) resolution
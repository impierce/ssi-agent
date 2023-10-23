# SSI Agent - Core

This crate contains the core business logic of the SSI Agent. It follows domain-driven design.

## Context: "Issuance"

This domain is all about the actual issuance of credentials.

### Domain Events

- `CredentialCreated`
- `CredentialSigned`
- `CredentialsOffered`
- `CredentialsOfferAccepted` // TODO: is selective acceptance possible?
- `CredentialsOfferDeclined`

### Commands

- `CreateCredential`
  - Payload
    - credential data format (w3c 1.1, openbadges 3.0)
    - _credential subject as raw json_
- `SignCredential`
  - Payload
    - optional: key id, fallback to default key (if set)
- `OfferCredentials`
  - Payload
    - optional: format (defaults to: OpenID4VCI)

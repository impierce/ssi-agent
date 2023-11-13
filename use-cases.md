# Use Cases

Issuance, Verification

## Issuance

### Templates

- import templates from external source?
- or define your own:
  - meta: @context, type, issuer, etc.
  - credentialSubject: needs to adhere to a schema

### Flow

1. use a template to create the raw credential (meta + subject)
   - default template: mandatory schema according to spec, credentialSubject is any valid JSON
2. sign the credential (approval required before offer? or before signature?)
   - default signature: use default key from key manager
3. create offer
   - default: pre-authorized, offer expiry set to 1 week

### DDD Entities

- `CredentialTemplate` (`CredentialSubjectSchema`)
- `CredentialData` (meta + subject)
- `Signature` (aka "adding trust to the data")
  - should this be an entity by itself or part of the "data"
    - pro: data should break signature if changed
    - con: data should be able to exist by itself?
- `CredentialOffer`
  - single or batch
  - optional: approval before offer is created
  - optional: expiry date

### Aggregate

- `Credential` consists of:
  - `CredentialTemplate` (is this part of the credential aggregate itself or is a template just the structure and the data actually copies the template values? --> explore pros & cons)
  - `CredentialData`
  - `Signature`

### Open questions

- Should it be possible to exchange the template AFTER subject has been filled? --> allows to issue credentials with a different format, but with the same content --> con: technically speaking, it's a different credential
  --> No, treated as

## Example flow: "Issue an Open Badge 3.0"

1. import a pre-defined `CredentialTemplate` (adhering to https://www.imsglobal.org/spec/ob/v3p0#abstract-0)
   (1.5. create identity (to be used as the issuer))
2. create `CredentialData` that adheres to the template (immutable) - if ever changed, create new credential (editing is platform-specific)
3. create a `Signature` for the given `CredentialData` with a default key from the key manager
4. Distribution (can be anything): create a `CredentialOffer` from the given `Credential` aggregate
5. destroy `CredentialOffer` after accept (aka "claim"), decline/reject, expiry (no claim in time)

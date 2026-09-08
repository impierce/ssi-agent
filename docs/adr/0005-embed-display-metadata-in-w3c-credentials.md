# ADR 0005: Embed Display Metadata (Name and Logo) in Root of W3C Credentials

**Status**: Accepted  
**Date**: 2026-09-08  
**Context**: Credential display metadata resolution in Linked Verifiable Presentations (Linked VPs)  

---

## Context

In standard OpenID4VCI issuance flows, digital wallets discover credential display properties (such as credential name, logo, colors, and descriptions) by querying the Credential Issuer Metadata endpoint (`/.well-known/openid-credential-issuer`).

However, when an issued credential is subsequently shared by a holder as a **Linked Verifiable Presentation (Linked VP)**, third-party verifiers and renderers inspect and verify the credential standalone out-of-band. Because the Linked VP specification is decoupled from OpenID4VCI issuer metadata endpoints, external verifiers and viewers have no standardized way to retrieve display metadata for rendering the credential.

Three main approaches were considered to address this:

1. **Embed display metadata (`name` and `logo`) directly in the credential root (Chosen)**:
   Extract `name` and `logo` from the credential configuration's display metadata at issuance time and embed them at the root of the unsigned W3C credential payload if not already provided.
2. **Link to the credential's corresponding UniTrust Template**:
   Embed a reference (URI or template identifier) pointing to the UniTrust Template that generated the credential.
3. **Link to a hosted JSON Schema (`credentialSchema`)**:
   Reference a valid, hosted JSON Schema URL (hosted by the application) that includes or links to schema-level display definitions.

---

## Decision

We chose **Option 1**: embed `name` and `logo` directly at the root of W3C credentials (`jwt_vc_json` and `vc+sd-jwt`) during credential construction if they are defined in the credential configuration's display metadata and not already present in the credential data payload.

---

## Rationale

- **Simplicity & Zero Infrastructure**: Option 1 requires no external schema hosting infrastructure, registry endpoints, or complex rendering pipelines.
- **Self-Describing Credentials**: External verifiers, public link viewers, and third-party tools can immediately render the credential name and logo without performing out-of-band HTTP requests.
- **Why not Link to UniTrust Templates (Option 2)**: UniTrust Templates are proprietary to our platform. Third-party verifiers outside the UniTrust ecosystem cannot resolve or interpret these templates, limiting interoperability.
- **Why not Hosted JSON Schemas (Option 3)**: Requiring the application to host, govern, and maintain high-availability JSON Schema endpoints with long-term URL stability introduces operational complexity and network coupling for verifiers that is unnecessary for basic display information.

---

## Consequences

- The credential payload size increases slightly due to embedding the logo object (`uri`, `alt_text`) and name.
- Display metadata becomes an immutable part of the signed credential; updating template branding post-issuance will not affect already-issued credentials.

---

## Future Work

In the future, we may revisit:
- Referencing a UniTrust Template URI for first-party/ecosystem verifiers that can leverage rich template display definitions.
- Linking to hosted JSON Schemas or standardized credential display specifications (such as OID4VCI Credential Display Definitions or W3C rendering extensions) once public schema hosting and standardized display profiles are established.

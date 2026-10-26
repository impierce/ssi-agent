# Integration Events Reference

<!-- This file is auto-generated from inline `///` doc comments on `#[derive(IntegrationEvent)]` enums. -->
<!-- Do not edit manually; run `cargo test generate_integration_events_docs` to regenerate. -->

All integration events are published as [CloudEvents v1.0](https://cloudevents.io/) JSON envelopes (`application/cloudevents+json`).

## Summary

| Event Type | Source | Subject | Description |
| :--- | :--- | :--- | :--- |
| `com.impierce.unicore.issuance.credential.offered` | `/services/issuance` | `offer_id` | Credential offer created and ready for holder interaction. |
| `com.impierce.unicore.issuance.offer.tx_code_generated` | `/services/issuance` | `offer_id` | Transaction code generated for a credential offer. |
| `com.impierce.unicore.issuance.offer.email_sent` | `/services/issuance` | `offer_id` | Credential offer delivery via email requested. |
| `com.impierce.unicore.issuance.credential.request_verified` | `/services/issuance` | `offer_id` | Holder credential request verified against an offer. |
| `com.impierce.unicore.issuance.credential.issued` | `/services/issuance` | `credential_id` | Verifiable credential cryptographically signed and issued. |
| `com.impierce.unicore.issuance.credential.revoked` | `/services/issuance` | `credential_id` | Credential status updated or revoked on the status list. |
| `com.impierce.unicore.issuance.credential.reissued` | `/services/issuance` | `reissuance_id` | Credential reissued from an existing credential and linked to a new offer. |
| `com.impierce.unicore.verification.presentation.requested` | `/services/verification` | `request_id` | OID4VP presentation exchange requested from holder. |
| `com.impierce.unicore.verification.presentation.verified` | `/services/verification` | `request_id` | Cryptographic presentation verified successfully. |
| `com.impierce.unicore.verification.presentation.failed` | `/services/verification` | `request_id` | Presentation verification failed or rejected. |
| `com.impierce.unicore.connection.invitation.created` | `/services/connection` | `connection_id` | Invitation created for peer connection. |
| `com.impierce.unicore.connection.established` | `/services/connection` | `connection_id` | Peer connection synced or accepted. |
| `com.impierce.unicore.template.created` | `/services/template` | `template_id` | Credential template created in library. |
| `com.impierce.unicore.template.title_updated` | `/services/template` | `template_id` | Credential template title updated. |
| `com.impierce.unicore.template.status_updated` | `/services/template` | `template_id` | Credential template lifecycle status updated. |
| `com.impierce.unicore.template.visibility_updated` | `/services/template` | `template_id` | Credential template visibility updated. |
| `com.impierce.unicore.template.display_updated` | `/services/template` | `template_id` | Credential template display configuration updated. |
| `com.impierce.unicore.template.tags_updated` | `/services/template` | `template_id` | Credential template tags updated. |
| `com.impierce.unicore.template.description_updated` | `/services/template` | `template_id` | Credential template description updated. |
| `com.impierce.unicore.template.type_updated` | `/services/template` | `template_id` | Credential template type values updated. |
| `com.impierce.unicore.template.schema_updated` | `/services/template` | `template_id` | Credential template JSON schema updated. |
| `com.impierce.unicore.template.schema_properties_attributes_updated` | `/services/template` | `template_id` | Credential template schema property attributes updated. |
| `com.impierce.unicore.template.credential_expiration_updated` | `/services/template` | `template_id` | Credential template expiration settings updated. |
| `com.impierce.unicore.template.holder_authorization_updated` | `/services/template` | `template_id` | Credential template holder authorization settings updated. |
| `com.impierce.unicore.template.deleted` | `/services/template` | `template_id` | Credential template deleted from library. |

## Payload Reference

### `IssuanceIntegrationEvent` (`/services/issuance`)

Public integration events emitted by the credential issuance bounded context.

#### `com.impierce.unicore.issuance.credential.offered`

Credential offer created and ready for holder interaction.

- **Variant**: `IssuanceIntegrationEvent::CredentialOffered`
- **CloudEvent `source`**: `/services/issuance`
- **CloudEvent `subject`**: `offer_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `offer_id` | `String` | Yes | Unique identifier of the credential offer. |
| `credential_ids` | `Vec<String>` | Yes | Configuration IDs of the credentials included in this offer. |
| `pre_authorized_code` | `String` | No | Pre-authorized code for OID4VCI token exchange, if applicable. |
| `tx_code` | `String` | No | Optional transaction PIN code required to redeem the offer. |
| `delivery_options` | `DeliveryOptions` | No | Optional out-of-band delivery options (such as recipient email). |

#### `com.impierce.unicore.issuance.offer.tx_code_generated`

Transaction code generated for a credential offer.

- **Variant**: `IssuanceIntegrationEvent::TxCodeGenerated`
- **CloudEvent `source`**: `/services/issuance`
- **CloudEvent `subject`**: `offer_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `offer_id` | `String` | Yes | Unique identifier of the credential offer. |
| `tx_code` | `String` | Yes | Generated transaction PIN code required to redeem the offer. |
| `delivery_options` | `DeliveryOptions` | No | Optional out-of-band delivery options (such as recipient email). |

#### `com.impierce.unicore.issuance.offer.email_sent`

Credential offer delivery via email requested.

- **Variant**: `IssuanceIntegrationEvent::CredentialOfferEmailSent`
- **CloudEvent `source`**: `/services/issuance`
- **CloudEvent `subject`**: `offer_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `offer_id` | `String` | Yes | Unique identifier of the credential offer. |
| `recipient_email` | `String` | Yes | Email address of the offer recipient. |
| `form_url_encoded_credential_offer` | `String` | Yes | URL-encoded OID4VCI credential offer URI. |
| `offer_link` | `Url` | Yes | Public landing page URL for redeeming the credential offer. |

#### `com.impierce.unicore.issuance.credential.request_verified`

Holder credential request verified against an offer.

- **Variant**: `IssuanceIntegrationEvent::CredentialRequestVerified`
- **CloudEvent `source`**: `/services/issuance`
- **CloudEvent `subject`**: `offer_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `offer_id` | `String` | Yes | Unique identifier of the credential offer being redeemed. |
| `subject_id` | `String` | No | DID or subject identifier of the holder, if authenticated. |

#### `com.impierce.unicore.issuance.credential.issued`

Verifiable credential cryptographically signed and issued.

- **Variant**: `IssuanceIntegrationEvent::CredentialIssued`
- **CloudEvent `source`**: `/services/issuance`
- **CloudEvent `subject`**: `credential_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `credential_id` | `String` | Yes | Unique identifier of the issued credential. |
| `status` | `String` | Yes | Lifecycle status of the credential (`"pending"` or `"issued"`). |

#### `com.impierce.unicore.issuance.credential.revoked`

Credential status updated or revoked on the status list.

- **Variant**: `IssuanceIntegrationEvent::CredentialRevoked`
- **CloudEvent `source`**: `/services/issuance`
- **CloudEvent `subject`**: `credential_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `credential_id` | `String` | Yes | Unique identifier of the revoked credential. |

#### `com.impierce.unicore.issuance.credential.reissued`

Credential reissued from an existing credential and linked to a new offer.

- **Variant**: `IssuanceIntegrationEvent::CredentialReissued`
- **CloudEvent `source`**: `/services/issuance`
- **CloudEvent `subject`**: `reissuance_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `reissuance_id` | `String` | Yes | Unique identifier of the reissuance record. |
| `original_credential_id` | `String` | Yes | Identifier of the original credential being superseded. |
| `new_credential_id` | `String` | Yes | Identifier of the newly created replacement credential. |
| `offer_id` | `String` | Yes | Identifier of the new credential offer created for the reissued credential. |

### `VerificationIntegrationEvent` (`/services/verification`)

Public integration events emitted by the core presentation verification bounded context.

#### `com.impierce.unicore.verification.presentation.requested`

OID4VP presentation exchange requested from holder.

- **Variant**: `VerificationIntegrationEvent::PresentationRequested`
- **CloudEvent `source`**: `/services/verification`
- **CloudEvent `subject`**: `request_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `request_id` | `String` | Yes | Unique identifier of the authorization request. |
| `client_id` | `String` | No | Relying party `client_id` associated with the request. |

#### `com.impierce.unicore.verification.presentation.verified`

Cryptographic presentation verified successfully.

- **Variant**: `VerificationIntegrationEvent::PresentationVerified`
- **CloudEvent `source`**: `/services/verification`
- **CloudEvent `subject`**: `request_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `request_id` | `String` | Yes | Unique identifier of the authorization request. |
| `validated` | `bool` | Yes | Whether the cryptographic presentation validation succeeded (`true`). |
| `state` | `String` | No | Optional OAuth2 `state` value echoed from the authorization response. |
| `presentations` | `DecodedVpToken` | No | Decoded verifiable presentations extracted from an OID4VP `vp_token` response. |

#### `com.impierce.unicore.verification.presentation.failed`

Presentation verification failed or rejected.

- **Variant**: `VerificationIntegrationEvent::PresentationFailed`
- **CloudEvent `source`**: `/services/verification`
- **CloudEvent `subject`**: `request_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `request_id` | `String` | Yes | Unique identifier of the authorization request. |
| `reason` | `String` | No | Human-readable reason for the verification failure. |

### `ConnectionIntegrationEvent` (`/services/connection`)

Integration events emitted by the Connection / Identity bounded context (`/services/connection`).

#### `com.impierce.unicore.connection.invitation.created`

Invitation created for peer connection.

- **Variant**: `ConnectionIntegrationEvent::InvitationCreated`
- **CloudEvent `source`**: `/services/connection`
- **CloudEvent `subject`**: `connection_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `connection_id` | `String` | Yes | Unique identifier of the connection record. |
| `url` | `String` | No | Optional endpoint URL of the peer connection. |

#### `com.impierce.unicore.connection.established`

Peer connection synced or accepted.

- **Variant**: `ConnectionIntegrationEvent::ConnectionEstablished`
- **CloudEvent `source`**: `/services/connection`
- **CloudEvent `subject`**: `connection_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `connection_id` | `String` | Yes | Unique identifier of the established connection record. |

### `TemplateIntegrationEvent` (`/services/template`)

Public integration events emitted by the credential template library bounded context.

#### `com.impierce.unicore.template.created`

Credential template created in library.

- **Variant**: `TemplateIntegrationEvent::TemplateCreated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the created template. |
| `title` | `String` | Yes | Human-readable title of the template. |
| `data_model` | `String` | Yes | Credential data model identifier (e.g. `"open_badges_3-0"`, `"w3c_vc_data_model_v2-0"`). |

#### `com.impierce.unicore.template.title_updated`

Credential template title updated.

- **Variant**: `TemplateIntegrationEvent::TitleUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |
| `title` | `String` | Yes | Updated human-readable title of the template. |

#### `com.impierce.unicore.template.status_updated`

Credential template lifecycle status updated.

- **Variant**: `TemplateIntegrationEvent::StatusUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |
| `status` | `String` | Yes | Updated lifecycle status (`"draft"`, `"published"`, `"archived"`, or `"deleted"`). |

#### `com.impierce.unicore.template.visibility_updated`

Credential template visibility updated.

- **Variant**: `TemplateIntegrationEvent::VisibilityUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |
| `visibility` | `String` | Yes | Updated visibility (`"private"` or `"public"`). |

#### `com.impierce.unicore.template.display_updated`

Credential template display configuration updated.

- **Variant**: `TemplateIntegrationEvent::DisplayUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |

#### `com.impierce.unicore.template.tags_updated`

Credential template tags updated.

- **Variant**: `TemplateIntegrationEvent::TagsUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |

#### `com.impierce.unicore.template.description_updated`

Credential template description updated.

- **Variant**: `TemplateIntegrationEvent::DescriptionUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |

#### `com.impierce.unicore.template.type_updated`

Credential template type values updated.

- **Variant**: `TemplateIntegrationEvent::TypeUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |

#### `com.impierce.unicore.template.schema_updated`

Credential template JSON schema updated.

- **Variant**: `TemplateIntegrationEvent::SchemaUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |

#### `com.impierce.unicore.template.schema_properties_attributes_updated`

Credential template schema property attributes updated.

- **Variant**: `TemplateIntegrationEvent::SchemaPropertiesAttributesUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |

#### `com.impierce.unicore.template.credential_expiration_updated`

Credential template expiration settings updated.

- **Variant**: `TemplateIntegrationEvent::CredentialExpirationUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |

#### `com.impierce.unicore.template.holder_authorization_updated`

Credential template holder authorization settings updated.

- **Variant**: `TemplateIntegrationEvent::HolderAuthorizationUpdated`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the updated template. |

#### `com.impierce.unicore.template.deleted`

Credential template deleted from library.

- **Variant**: `TemplateIntegrationEvent::TemplateDeleted`
- **CloudEvent `source`**: `/services/template`
- **CloudEvent `subject`**: `template_id`

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template_id` | `String` | Yes | Unique identifier of the deleted template. |


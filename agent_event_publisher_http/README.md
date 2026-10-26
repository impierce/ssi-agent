# agent_event_publisher_http

A simple HTTP event publisher for the SSI Agent.

To make use of this publisher you need to configure it by adding one or more entries to the `http` array in your configuration file.

- The `target_url` is the URL to which the events will be published.
- The `events.types` list specifies which integration event types or patterns will be forwarded to the `target_url`.

Example:

```yaml
event_publishers:
  http:
    - enabled: true
      target_url: "https://my-domain.example.org/event-subscriber"
      headers:
        authorization: Basic YWxhZGRpbjpvcGVuc2VzYW1l
      events:
        types:
          - "com.impierce.unicore.issuance.credential.offered"
          - "com.impierce.unicore.issuance.credential.issued"
    - enabled: false
      target_url: "https://another-endpoint.example.org/events"
      events:
        types:
          - "com.impierce.unicore.verification.presentation.verified"
```

### Request format

The events will be sent as a POST request with the CloudEvent v1.0 formatted JSON in the body.

Example:

```http
POST /<target_url>
Content-Type: application/cloudevents+json

{
  "specversion": "1.0",
  "id": "credential:1c69e4cb-e75f-4f56-9418-f46cb441e639:1",
  "source": "/services/issuance",
  "type": "com.impierce.unicore.issuance.credential.issued",
  "subject": "1c69e4cb-e75f-4f56-9418-f46cb441e639",
  "datacontenttype": "application/json",
  "time": "2026-10-06T18:00:00Z",
  "callerid": "u-1",
  "callertype": "user",
  "data": {
    "credential_id": "1c69e4cb-e75f-4f56-9418-f46cb441e639",
    "status": "issued"
  }
}
```

### Available integration events

| Event Type | Source | Subject | Description |
| :--- | :--- | :--- | :--- |
| `com.impierce.unicore.issuance.credential.offered` | `/services/issuance` | Offer ID | Credential offer created and ready for holder interaction. |
| `com.impierce.unicore.issuance.offer.tx_code_generated` | `/services/issuance` | Offer ID | Transaction code generated for a credential offer. |
| `com.impierce.unicore.issuance.offer.email_sent` | `/services/issuance` | Offer ID | Credential offer delivery via email requested. |
| `com.impierce.unicore.issuance.credential.request_verified` | `/services/issuance` | Offer ID | Holder credential request verified against offer. |
| `com.impierce.unicore.issuance.credential.issued` | `/services/issuance` | Credential ID | Verifiable credential cryptographically signed and issued. |
| `com.impierce.unicore.issuance.credential.revoked` | `/services/issuance` | Credential ID | Credential status updated or revoked. |
| `com.impierce.unicore.issuance.credential.reissued` | `/services/issuance` | Reissuance ID | Credential reissued from an existing credential. |
| `com.impierce.unicore.verification.presentation.requested` | `/services/verification` | Request ID | Presentation exchange requested from holder. |
| `com.impierce.unicore.verification.presentation.verified` | `/services/verification` | Request ID | Cryptographic presentation verified successfully. |
| `com.impierce.unicore.verification.presentation.failed` | `/services/verification` | Request ID | Presentation verification failed or rejected. |
| `com.impierce.unicore.connection.invitation.created` | `/services/connection` | Connection ID | Invitation created for peer connection. |
| `com.impierce.unicore.connection.established` | `/services/connection` | Connection ID | Peer connection synced or accepted. |
| `com.impierce.unicore.template.created` | `/services/template` | Template ID | Credential template created in library. |
| `com.impierce.unicore.template.deleted` | `/services/template` | Template ID | Credential template deleted from library. |
| `com.impierce.unicore.template.title_updated` | `/services/template` | Template ID | Credential template title updated. |
| `com.impierce.unicore.template.description_updated` | `/services/template` | Template ID | Credential template description updated. |
| `com.impierce.unicore.template.display_updated` | `/services/template` | Template ID | Credential template display configuration updated. |
| `com.impierce.unicore.template.tags_updated` | `/services/template` | Template ID | Credential template tags updated. |
| `com.impierce.unicore.template.status_updated` | `/services/template` | Template ID | Credential template status updated. |
| `com.impierce.unicore.template.visibility_updated` | `/services/template` | Template ID | Credential template visibility updated. |
| `com.impierce.unicore.template.type_updated` | `/services/template` | Template ID | Credential template type values updated. |
| `com.impierce.unicore.template.schema_updated` | `/services/template` | Template ID | Credential template JSON schema updated. |
| `com.impierce.unicore.template.schema_properties_attributes_updated` | `/services/template` | Template ID | Credential template schema property attributes updated. |
| `com.impierce.unicore.template.credential_expiration_updated` | `/services/template` | Template ID | Credential template expiration settings updated. |
| `com.impierce.unicore.template.holder_authorization_updated` | `/services/template` | Template ID | Credential template holder authorization settings updated. |

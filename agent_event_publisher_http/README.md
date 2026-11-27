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

# agent_event_publisher_nats

A simple NATS event publisher for the SSI Agent.

## Configuration

To make use of this publisher you need to configure it by adding the `nats` object to your configuration file.

```yaml
event_publishers:
  nats:
    enabled: true
    nats_url: "nats://localhost:4222" # NATS server URL
    subjects:
      - name: "email.commands" # NATS subject to publish to
        events:
          types:
            - "com.impierce.unicore.issuance.credential.offered"
```

## Usage

### 1. Load and spawn the publisher

```rust
use agent_event_publisher_nats::NatsEventPublisher;

if let Some(nats_publisher) = NatsEventPublisher::from_config(&integration_event_bus).await? {
    nats_publisher.spawn();
}
```

The publisher subscribes to the shared kernel `EventBusHandle` and automatically forwards matching `CloudEvent`s to configured NATS subjects.
Using the example configuration, matching events are published to `"email.commands"`.

### 2. Example published event

When a `com.impierce.unicore.issuance.credential.offered` integration event occurs, it publishes a CloudEvent to the defined subject.
For example:

```json
{
  "specversion": "1.0",
  "type": "com.impierce.unicore.issuance.credential.offered",
  "source": "/services/issuance",
  "id": "offer:12345:1",
  "subject": "12345",
  "datacontenttype": "application/json",
  "data": {
    "offer_id": "12345",
    "credential_ids": ["cred-1"],
    "pre_authorized_code": "code-abc"
  }
}
```

### Available integration events

See [`agent_event_publisher_http/README.md`](../agent_event_publisher_http/README.md) for the complete catalog of Published Language integration events (`com.impierce.unicore.*`).

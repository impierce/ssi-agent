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
          types: [TxCodeGenerated, CredentialOfferEmailSent] # Event types to publish
```

## Usage

### 1. Load and spawn the publisher

```rust
use agent_event_publisher_nats::NatsEventPublisher;

if let Some(nats_publisher) = NatsEventPublisher::from_config(&event_bus).await? {
    nats_publisher.spawn();
}
```

The publisher subscribes to the shared kernel `EventBusHandle` and automatically forwards matching `CloudEvent`s to configured NATS subjects.
Using the example configuration, matching events are published to "email.commands".

### 2. Example published event

When a `TxCodeGenerated` event occurs, it publishes a CloudEvent to the defined subject.
For example:

```json
{
  "specversion": "1.0",
  "type": "com.impierce.unicore.tx-code-generated",
  "source": "/services/offer",
  "id": "offer:12345:1",
  "datacontenttype": "application/json",
  "data": {
    "offer_id": "12345",
    "tx_code": "1234",
    "delivery_options": {
      "recipient_email": "user@example.com"
    }
  }
}
```

### Available events

#### `offer`

TxCodeGenerated, CredentialOfferEmailSent

# agent_event_publisher_nats

A simple NATS event publisher for the SSI Agent.
To make use of this publisher you need to configure it by adding the `nats` object to your configuration file.

## Configuration

Add the `nats` configuration to your `config.yaml`:

```yaml
event_publishers:
  nats:
    enabled: true
    nats_url: "nats://localhost:4222" # NATS server URL
    subject: "email.commands" # NATS subject to publish to
    events:
      offer:
        - "TxCodeGenerated" # Event types to publish
```

## Usage

### 1. Load the publisher

```rust
use agent_event_publisher_nats::EventPublisherNats;

let nats_publisher = EventPublisherNats::load().await?;
```

The publisher implements the `Query<Offer>` trait and will automatically publish events when they occur.

### 3. Example published event

When a `TxCodeGenerated` event occurs, it publishes a CloudEvent like:

```json
{
  "specversion": "1.0",
  "type": "email.command.txcode.generated",
  "source": "https://example.com/event",
  "id": "offer-123-uuid",
  "datacontenttype": "application/json",
  "data": {
    "recipient_email": "user@example.com",
    "template": "transaction_code",
    "values": "ABC123"
  }
}
```

### Available events

Currently, the nats publisher only listens for and publishes in response to one event:

#### `offer`

TxCodeGenerated

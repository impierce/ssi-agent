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
        offer:
          - "TxCodeGenerated" # Event types to publish
```

## Usage

### 1. Load the publisher

```rust
use agent_event_publisher_nats::EventPublisherNats;

let nats_publisher = EventPublisherNats::load().await?;
```

The publisher currently implements the `Query<Offer>` trait and will automatically publish those events when they occur.
Using example configuration, NATS will publish the event to "email.commands".

### 3. Example published event

When a `TxCodeGenerated` event occurs, it publishes a CloudEvent to the defined subject.
For example:

```json
{
  "specversion": "1.0",
  "type": "email.command.txcodegenerated",
  "source": "https://example.com/event",
  "id": "offer-123-uuid",
  "datacontenttype": "application/json",
  "data": {
    "TxCodeGenerated": {
      "offer_id": "12345",
      "tx_code": "1234",
      "delivery_options": {
        "recipient_email": "user@example.com"
      }
    }
  }
}
```

### Available events

#### `offer`

TxCodeGenerated, CredentialOfferEmailSent

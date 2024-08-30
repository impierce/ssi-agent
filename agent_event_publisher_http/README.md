# agent_event_publisher_http

A simple HTTP event publisher for the SSI Agent.

To make use of this publisher you need to configure it by adding the `http` object to your configuration file.

- The `target_url` is the URL to which the events will be published.
- The `target_events` is a list of events that will be published to the `target_url`.

Example:

```yaml
event_publishers:
  http:
    enabled: false
    target_url: "https://my-domain.example.org/event-subscriber"
    events:
      server_config: []
      credential: [UnsignedCredentialCreated, CredentialSigned]
```

### Available events

#### `credential`

```
UnsignedCredentialCreated
SignedCredentialCreated
CredentialSigned
```

#### `offer`

```
CredentialOfferCreated
CredentialsAdded
FormUrlEncodedCredentialOfferCreated
TokenResponseCreated
CredentialRequestVerified
CredentialResponseCreated
```

#### `server_config`

```
ServerMetadataLoaded
CredentialConfigurationAdded
```

#### `holder_credential`

```
CredentialAdded
```

#### `received_offer`

```
CredentialOfferReceived
CredentialOfferAccepted
TokenResponseReceived
CredentialResponseReceived
CredentialOfferRejected
```

#### `authorization_request`

```
AuthorizationRequestCreated
FormUrlEncodedAuthorizationRequestCreated
AuthorizationRequestObjectSigned
```

#### `connection`

```
SIOPv2AuthorizationResponseVerified
OID4VPAuthorizationResponseVerified
```

## Leveraging Just-in-Time Data Request Events

UniCore facilitates dynamic integration with external systems through just-in-time data request events, dispatched seamlessly via the HTTP Event Publisher. This enables real-time data retrieval and on-demand generation, enhancing flexibility and efficiency in your SSI ecosystem.

### Example Scenarios

**Custom Credential Signing**

UniCore facilitates the utilization of just-in-time data request events for customized credential signing workflows. This approach enables users to manage the signing process independently, offering greater control over credential issuance. When UniCore verifies a Credential Request from a Wallet, it triggers the `CredentialRequestVerified` event. By utilizing the HTTP Event Publisher, this event, containing essential identifiers like `offer_id` and `subject_id`, can be dispatched to external systems. Subsequently, external systems leverage these identifiers to generate and sign credentials, which are then submitted to UniCore's `/v0/credentials` endpoint.

To integrate just-in-time data request events into your workflow, adhere to the following steps:

1. Configure the HTTP Event Publisher to listen for the `CredentialRequestVerified` event. The following configuration
   can be added to your `config.yaml` file:
  ```yaml
  event_publishers:
    http:
      enabled: true
      target_url: "https://your-server.org/event-subscriber"
      events:
        offer: [CredentialRequestVerified]
  ```
2. The above configuration makes sure that whenever a Wallet sends a Credential Request, the HTTP Event Publisher will
  dispatch the `CredentialRequestVerified` event to the specified URL once it successfully verified the Credential
  Request, e.g:
  ```json
  POST /event-subscriber HTTP/1.1
  Host: https://your-server.org
  Content-Type: application/json
  Content-Length: 328
  {
    "CredentialRequestVerified": {
      "offer_id": "001",
      "subject_id": "did:jwk:eyJhbGciOiJFUzI1NiIsImNydiI6IlAtMjU2Iiwia2lkIjoieERDQVBRbHRVa2JZMnByTkdpT0ItNWJ2T0pnZnQ0NVJqYjM2RWNjSWNGdyIsImt0eSI6IkVDIiwieCI6Im02b3EySFF6NmluSk8xbzg1VUM5VVEyamxJRFJld0ROVS0ybUktVThKN1UiLCJ5Ijoia0NwbTcwbXpCT3Y0OWFPdHdmRUdxVW1fSkllWXlZeWdWSXpKaFpXY1ZnTSJ9"
    }
  }
  ```
3. Now your system can apply its own logic and create and sign a Credential based on the data received from the Event.
   The signed Credential can then be submitted to UniCore's `/v0/credentials` endpoint, e.g:
  ```json
  POST /v0/credentials HTTP/1.1
  Host: https://unicore-server.org
  Content-Type: application/json
  Content-Length: 328
  {
    "offerId": "001",
    "credential": "<the-signed-credential>",
    "isSigned": true,
    "credentialConfigurationId": ""
  }
  ```
4. Once UniCore receives the signed Credential, it will finalize the issuance process by embedding the signed Credential
   into the Credential Response to the Wallet.

By default, UniCore will wait up to 1000 ms for the signed credential to arrive. This parameter can be changed by
setting the `AGENT_API_REST_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS` environment variable.

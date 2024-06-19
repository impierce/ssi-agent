# agent_event_publisher_http

A simple HTTP event publisher for the SSI Agent.

To make use of this publisher you need to configure it by creating a `config.yaml` file in this same directory. For each
aggregate you want to publish events for, you need to set the following properties:

- The `target_url` is the URL to which the events will be published.
- The `target_events` is a list of events that will be published to the `target_url`.

Example `config.yaml`:

```yaml
target_url: &target_url "https://my-domain.example.org/ssi-event-subscriber"

connection: {
  target_url: *target_url,
  target_events: [
    SIOPv2AuthorizationResponseVerified
  ]
}
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
CredentialAdded
FormUrlEncodedCredentialOfferCreated
TokenResponseCreated
CredentialRequestVerified
CredentialResponseCreated
```

#### `server_config`

```
ServerMetadataLoaded
CredentialConfigurationCreated
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

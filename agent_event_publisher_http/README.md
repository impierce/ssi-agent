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

#### `connection`

```
ConnectionAdded
```

#### `document`

```
DocumentCreated
ServiceAdded
```

#### `service`

```
DomainLinkageServiceCreated
LinkedVerifiablePresentationServiceCreated
```

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

#### `presentation`

```
PresentationCreated
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
SIOPv2AuthorizationResponseVerified
OID4VPAuthorizationResponseVerified
```

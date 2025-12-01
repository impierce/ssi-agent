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

#### `access_token`

```
AccessTokenIssued
```

#### `authorization_code`

```
AuthorizationCodeCreated
AuthorizationCodeRedeemed
```

#### `client`

```
ClientRegistered
```

#### `oauth2_authorization_request`

```
OAuth2AuthorizationRequestCreated
OAuth2AuthorizationRequestExpired
ConsentGranted
ConsentRejected
```

#### `connection`

```
ConnectionAdded
```

#### `document`

```
DocumentCreated
PublicKeyUpdated
DocumentStatusUpdated
ServiceAdded
DocumentPublished
```

#### `profile`

```
ProfileCreated,
DisplayNameUpdated,
LogoUpdated,
CountryUpdated,
SourceUpdated,
```

#### `service`

```
DomainLinkageServiceCreated
DomainLinkageServiceDeleted
LinkedVerifiablePresentationServiceCreated
```

#### `template`

```
TemplateCreated
TitleUpdated
DisplayUpdated
CredentialFormatUpdated
CreatorUpdated
HolderTypeUpdated
TagsUpdated
StatusUpdated
VisibilityUpdated
DescriptionUpdated
TypeUpdated
SchemaUpdated
```

#### `server_config`

```
ServerMetadataLoaded
CredentialConfigurationUpdated
```

#### `credential`

```
UnsignedCredentialCreated
SignedCredentialCreated
CredentialSigned
NotificationReceived
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

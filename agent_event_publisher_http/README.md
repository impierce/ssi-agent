# agent_event_publisher_http

A simple HTTP event publisher for the SSI Agent.

To make use of this publisher you need to configure it by adding one or more entries to the `http` array in your configuration file.

- The `target_url` is the URL to which the events will be published.
- The `target_events` is a list of events that will be published to the `target_url`.

Example:

```yaml
event_publishers:
  http:
    - enabled: true
      target_url: "https://my-domain.example.org/event-subscriber"
      headers:
        authorization: Basic YWxhZGRpbjpvcGVuc2VzYW1l
      events:
        server_config: []
        credential: [UnsignedCredentialCreated, CredentialSigned]
    - enabled: false
      target_url: "https://another-endpoint.example.org/events"
      events:
        offer: [CredentialOfferCreated]
```

### Request format

The events will be sent as a POST request with the event as JSON in the body.

Example:

```http
POST /<target_url>
Content-Type: application/json

{
  "SignedCredentialCreated": {
    "credential_id": "1c69e4cb-e75f-4f56-9418-f46cb441e639",
    "signed_credential": [...]
    "notification_id": "6327889a-7c72-4ba3-bb84-f7da3c3d82cb"
  }
}
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
DescriptionUpdated,
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
DataModelUpdated
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

#### `nonce`

```
NonceGenerated
NonceRedeemed
```

#### `status_list`

```
StatusListCreated
IndexAdded
IndexUpdated
```

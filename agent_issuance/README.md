# agent_issuance

This module contains business logic for issuing credentials. This ranges from using a credential template,
applying user-specific subject data to it and offering the credential to a user wallet via the [OpenID4VCI](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html) standard protocol.

## Configuration

The `agent_issuance` module is configured via the `issuance-config.yml` file. The following properties are available:

- `server_config`: **REQUIRED** The server configuration for Issuance. It contains the following properties:
  - `credential_configurations`: **REQUIRED** An array of Credential Configurations. As of now, UniCore **requires the
    array to contain exactly one Credential Configuration**. The Credential Configuration has the following properties:
    - `credential_configuration_id`: **REQUIRED** The ID of the Credential Configuration. This ID will be used to
      reference the Credential Configuration in the HTTP API's `/v0/credentials` endpoint.
    - `format`: **REQUIRED** The format of the Credential. As of now, UniCore only supports `jwt_vc_json`.
    - `credential_definition`: **REQUIRED** An object describing the properties of the Credentials that will be
      issued. This object contains the following properties:
      - `type`: **REQUIRED** an array of strings that describe the type of the Credential.
      - `credentialSubject`: **OPTIONAL** an object that describes the properties of the Credential Subject. For
        more information, see the [OpenID4VCI
        specification](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0-13.html#appendix-A.1.1.2-3.1.2.2.1)
    - `display`: **OPTIONAL** An object describing the display properties of the to be issued Credentials. This
      object contains the following properties:
      - `name`: **REQUIRED** The name of the Credential.
      - `locale`: **OPTIONAL** The locale of the Credential.
      - `logo`: **OPTIONAL** The logo properties of the to be issued Credentials. This object contains the
        following properties:
        - `url`: **REQUIRED** The URL of the logo.
        - `alt_text`: **OPTIONAL** String that describes the logo.

Example of configuration options in `issuance-config.yml`:

```yaml
server_config:
  credential_configurations:
    - credential_configuration_id: w3c_vc_credential
      format: jwt_vc_json
      credential_definition:
        type:
          - VerifiableCredential
          # - OpenBadgeCredential <-- uncomment this to specify that the to be issued Credentials are OpenBadgev3 Credentials
      display:
        - name: Verifiable Credential
          locale: en
          logo:
            uri: https://impierce.com/images/logo-blue.png
            alt_text: UniCore Logo
```

### Credential expiration

You are required to set the expiration of a credential by providing an expiration date after which the credential should be rejected by verifiers.

Expiration dates need to contain date and time (relative to UTC) following the [ISO 8601](https://en.wikipedia.org/wiki/ISO_8601) standard, such as `2024-12-09T10:15:00Z`.

> If you want the credential to be valid forever, you can set the expiration to `never`. The `exp` claim in the JWT will then be omitted.

### Credential Offer Delivery

Credential offers can be delivered either B2C (via a recipient email) or B2B between other business Wallets via a Target Url. These delivery options are specified under the DeliveryMethod field in offer issuance.

When email delivery is selected, the issuance publishes a `CredentialOfferEmailSent` event containing the recipient email and credential offer details. External email delivery services can subscribe to these events and handle the actual email transmission.

### Credential Types and Verifiable Formats

Credentials can be issued in 4 different **Digital Credential Data Formats**:
- Verifiable Credential Data Model 1.1 (VC DM 1.1)
- Verifiable Credential Data Model 2.0 (VC DM 2.0)
- Open Badges 3.0 (OB 3.0)
- European Digital Credential 3.3 (ELM 3.3)
> For more resources on these specifications please refer to the `README.md` file in the `agent_library/src/json_schemas` folder.

Furthermore, there are 3 supported formats in which these credentials can be made verifiable, all of which are **envelopping methods**:
- [W3C Verifiable Credentials 1.1](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-w3c-verifiable-credentials) (`jwt_vc_json`)
- [SD-JWT VC](https://datatracker.ietf.org/doc/draft-ietf-oauth-sd-jwt-vc/) (`dc+sd-jwt`)
- [VC DM 2.0 SD-JWT](https://www.w3.org/TR/vc-jose-cose/#with-sd-jwt) (`vc+sd-jwt`)

Due to certain restrictions in the above mentioned formats, not all data formats can be combined with each envelopping method. The following combinations are possible:
- W3C Verifiable Credentials 1.1 - VC DM 1.1
- W3C Verifiable Credentials 1.1 - OB 3.0 (backwards compatible)
- W3C Verifiable Credentials 1.1 - ELM 3.3
- SD-JWT VC (this is a complete Credential Data Format and envelopping method of its own)
- VC DM 2.0 SD-JWT - VC DM 2.0
- VC DM 2.0 SD-JWT - OB 3.0
- VC DM 2.0 SD-JWT - ELM 3.3 (forward compatible)
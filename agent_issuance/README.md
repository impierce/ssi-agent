# agent_issuance

This module contains business logic for issuing credentials. This ranges from using a credential template,
applying user-specific subject data to it and offering the credential to a user wallet via the [OpenID4VCI](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html) standard protocol.


## Configuration

The `agent_issuance` module is configured via the `issuance-config.yml` file. The following properties are available:
* `server_config`: **REQUIRED** The server configuration for Issuance. It contains the following properties:
    * `credential_configurations`: **REQUIRED** An array of Credential Configurations. As of now, UniCore **requires the
      array to contain exactly one Credential Configuration**. The Credential Configuration has the following properties:
        * `credential_configuration_id`: **REQUIRED** The ID of the Credential Configuration. This ID will be used to
          reference the Credential Configuration in the REST API's `/v0/credentials` endpoint.
        * `format`: **REQUIRED** The format of the Credential. As of now, UniCore only supports `jwt_vc_json`.
        * `credential_definition`: **REQUIRED** An object describing the properties of the Credentials that will be
          issued. This object contains the following properties:
            * `type`: **REQUIRED** an array of strings that describe the type of the Credential.
            * `credentialSubject`: **OPTIONAL** an object that describes the properties of the Credential Subject. For
              more information, see the [OpenID4VCI
              specification](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0-13.html#appendix-A.1.1.2-3.1.2.2.1)
        * `display`: **OPTIONAL** An object describing the display properties of the to be issued Credentials. This
          object contains the following properties:
            * `name`: **REQUIRED** The name of the Credential.
            * `locale`: **OPTIONAL** The locale of the Credential.
            * `logo`: **OPTIONAL** The logo properties of the to be issued Credentials. This object contains the
              following properties:
                * `url`: **REQUIRED** The URL of the logo.
                * `alt_text`: **OPTIONAL** String that describes the logo.

Example of configuration options in `issuance-config.yml`:
```yaml
server_config:
  credential_configurations:
    - credential_configuration_id: w3c_vc_credential
      format: jwt_vc_json
      credential_definition:
        type:
          - VerifiableCredential
      display:
        - name: Verifiable Credential
          locale: en
          logo:
            uri: https://impierce.com/images/logo-blue.png
            alt_text: UniCore Logo
```

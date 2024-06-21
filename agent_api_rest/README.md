# agent_api_rest

A lightweight REST API for the SSI Agent.

### OpenAPI specification (Swagger UI)

```bash
docker run --rm -p 9090:8080 -e SWAGGER_JSON=/tmp/openapi.yaml -v $(pwd):/tmp swaggerapi/swagger-ui
```

Browse to http://localhost:9090

### CORS

If you want to access UniCore's API from a browser, you can set the `AGENT_APPLICATION_ENABLE_CORS` environment variable to `true`. This will enable a permissive CORS policy (allow all).

## Usage
Below we describe a typical usage of the REST API for UniCore. 

### Issuance

#### Creating new/overwriting existing Credential Configuration

<details>
 <summary><code>POST</code> <code><b>/configurations/credential_configurations</b></code></summary>

The Credential Configuration is a JSON object that defines the format of the credentials that will be issued. This
typically only needs to be configured once and will ensure that through the `credentialConfigurationId` [Credentials can
be created and issued](#creating-a-new-credential) accordingly. 

##### Parameters
- `credentialConfigurationId`: **REQUIRED** This identifier can be used to refer to the Credential Configuration when creating a new
  Credential through the `/v1/credentials` endpoint.
- `format`: **REQUIRED** The format of the Credential. Currently, only `jwt_vc_json` is supported.
- `credential_definition`: **REQUIRED** An object describing the Credentials that will be issued through this Credential
  Configuration.
    - `type`: **REQUIRED** The type of Credentials that will be issued through this Credential Configuration.
    - `credentialSubject`: **OPTIONAL** This parameter can be used to add a more precise description of the Credentials.
- `display`: **OPTIONAL** An array of objects that describe how the Credential will be displayed in the UniCore Wallet.
    - `name`: **REQUIRED** The name of the Credential.
    - `locale`: **OPTIONAL** The locale of the Credential.
    - `logo`: **OPTIONAL** An object that describes the logo of the Credential.
        - `url`: **REQUIRED** The URL of the logo.
        - `alt_text`: **OPTIONAL** The alt text of the logo.

```json
{
    "credentialConfigurationId":"w3c_vc_credential",
    "format": "jwt_vc_json",
    "credential_definition": {
        "type": [
            "VerifiableCredential",
            // "OpenBadgeCredential"
        ]
    },
    "display": [{
        "name": "Identity Credential",
        "locale": "en",
        "logo": {
            "url": "https://impierce.com/images/logo-blue.png",
            "alt_text": "UniCore Logo"
        }
    }]
}
```

</details>

#### Creating a new Credential

<details>
 <summary><code>POST</code> <code><b>/credentials</b></code></summary>

Now there is a new Credential Configuration, we can create our first Credential. 

##### Parameters
- `offerId`: **REQUIRED**: A unique identifier for the Credential Offer. This ID will bind the Credential to the
  [Credential Offer that we will receive later](#retrieving-the-percent-encoded-credential-offer)
- `credentialConfigurationId`: **REQUIRED** 
- `credential`: **REQUIRED** An object containing the data that will be included in the Credential. This data should
  adhere to the Credential Definition that was defined in the Credential Configuration.

```json
{
    "offerId":"my-first-offer",
    "credentialConfigurationId": "w3c_vc_credential",
    "credential": {
        "credentialSubject": {
            "first_name": "Ferris",
            "last_name": "Crabman",
            "dob": "1982-01-01"
        }
    }
}
```

</details>

#### Retrieving the percent-encoded Credential Offer

<details>
 <summary><code>POST</code> <code><b>/offers</b></code></summary>

After creating a new Credential, we can retrieve the Credential Offer. The Credential Offer is a percent-encoded string
that can be rendered as a QR-Code which in turn can be scanned with an [Identity Wallet](https://github.com/impierce/identity-wallet).

##### Parameters
- `offerId`: **REQUIRED**: The ID of the Credential Offer 

```json
{
    "offerId":"my-first-offer"
}
```

</details>

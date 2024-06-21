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
Typical usage of the Issuance of Credentials

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
  [Credential Offer that we will receive later](#retrieving-the-url-encoded-credential-offer)
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

#### Retrieving the url-encoded Credential Offer

<details>
 <summary><code>POST</code> <code><b>/offers</b></code></summary>

After creating a new Credential, we can retrieve the Credential Offer. The Credential Offer is a url-encoded string
that can be rendered as a QR-Code which in turn can be scanned with an [Identity Wallet](https://github.com/impierce/identity-wallet).

##### Parameters
- `offerId`: **REQUIRED**: The ID of the Credential Offer 

```json
{
    "offerId":"my-first-offer"
}
```

</details>

### Verification
Typical usage of the Verification of Authorization Responses.

#### Creating a url-encoded SIOPv2 Authorization Request

<details>
 <summary><code>POST</code> <code><b>/authorization_request</b></code></summary>

Through this endpoint, we can create a url-encoded Authorization Request that can be rendered as a QR-Code. This
QR-Code can be scanned by an [Identity Wallet](https://github.com/impierce/identity-wallet) which in turn will answer the Authorization Request.

##### Parameters
- `nonce`: **REQUIRED**: A unique identifier for the Authorization Request.
- `state`: **OPTIONAL**: A unique string representing the state of the Authorization Request.

```json
{
    "nonce":"this is a nonce"
}
```

</details>

#### Creating a url-encoded OID4VP Authorization Request

<details>
 <summary><code>POST</code> <code><b>/authorization_request</b></code></summary>

Through this endpoint, we can create a url-encoded Authorization Request that can be rendered as a QR-Code. This
QR-Code can be scanned by an [Identity Wallet](https://github.com/impierce/identity-wallet) which in turn will answer
the Authorization Request. The only extra required parameter is the `presentation_definition` which describes the
Verifiable Credential(s) that will be requested from the Identity Wallet.

##### Parameters
- `nonce`: **REQUIRED**: A unique identifier for the Authorization Request.
- `presentation_definition`: An object describing the Verifiable Credential(s) that will be requested from the Identity
  Wallet to ensure a successful Authorization. In most cases, the `presentation_definition` below will 
- `state`: **OPTIONAL**: A unique string representing the state of the Authorization Request.

```json
{
    "nonce": "this is a nonce",
    "presentation_definition": {
        "id":"Verifiable Presentation request for sign-on",
        "input_descriptors":[
            {
                "id":"Request for Verifiable Credential",
                "constraints":{
                    "fields":[
                        {
                            "path":[
                                "$.vc.type"
                            ],
                            "filter":{
                                "type":"array",
                                "contains":{
                                    "const":"VerifiableCredential"
                                    // "const":"OpenBadgesCredential" <-- for OpenBadges
                                }
                            }
                        },
                        // Extra constraints can be added to the Presentation Definition.
                        // {
                        //     "path":[
                        //         "$.vc.credentialSubject.first_name"
                        //     ]
                        // },
                        // {
                        //     "path":[
                        //         "$.vc.credentialSubject.last_name"
                        //     ]
                        // },
                    ]
                }
            }
        ]
    }
}
```

</details>

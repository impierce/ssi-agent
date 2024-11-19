# agent_api_rest

A lightweight REST API for the SSI Agent.

UniCore's REST API is currently still in the pre-release stage meaning that the API is still under active development.
Breaking changes may occur before the API reaches a stable version.

The current version of the REST API is `v0`.

### OpenAPI specification (Swagger UI)

```bash
docker run --rm -p 9090:8080 -e SWAGGER_JSON=/tmp/openapi.yaml -v $(pwd):/tmp swaggerapi/swagger-ui
```

Browse to http://localhost:9090

### CORS

If you want to access UniCore's API from a browser, you can set the `UNICORE__CORS_ENABLED` environment variable to `true`. This will enable a permissive CORS policy (allow all).

## Usage

Below we describe a typical usage of the REST API for UniCore.

### Issuance

Typical usage of the Issuance of Credentials

</details>

#### Creating a new Credential

<details>
 <summary><code>POST</code> <code><b>/v0/credentials</b></code></summary>

Now there is a new Credential Configuration, we can create our first Credential.

##### Parameters

- `offerId`: **REQUIRED**: A unique identifier for the Credential Offer. This ID will bind the Credential to the
  [Credential Offer that we will receive later](#retrieving-the-URL-encoded-credential-offer)
- `credentialConfigurationId`: **REQUIRED**
- `credential`: **REQUIRED** An object containing the data that will be included in the Credential. This data should
  adhere to the Credential Definition that was defined in the Credential Configuration. See the [Issuance
  Configuration](../agent_issuance/README.md) for more information about how the Credential Configuration is defined.

```json
{
  "offerId": "my-first-offer",
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

#### Retrieving the URL-encoded Credential Offer

<details>
 <summary><code>POST</code> <code><b>/v0/offers</b></code></summary>

After creating a new Credential, we can retrieve the Credential Offer. The Credential Offer is a URL-encoded string
that can be rendered as a QR-Code which in turn can be scanned with an [Identity Wallet](https://github.com/impierce/identity-wallet).

##### Parameters

- `offerId`: **REQUIRED**: The ID of the Credential Offer

```json
{
  "offerId": "my-first-offer"
}
```

</details>

### Verification

Typical usage of the Verification of Authorization Responses.

#### Creating a URL-encoded SIOPv2 Authorization Request

<details>
 <summary><code>POST</code> <code><b>/v0/authorization_request</b></code></summary>

Through this endpoint, we can create a URL-encoded Authorization Request that can be rendered as a QR-Code. This
QR-Code can be scanned by an [Identity Wallet](https://github.com/impierce/identity-wallet) which in turn will answer the Authorization Request.

##### Parameters

- `nonce`: **REQUIRED**: A unique identifier for the Authorization Request.
- `state`: **OPTIONAL**: A unique string representing the state of the Authorization Request.

```json
{
  "nonce": "this is a nonce"
}
```

</details>

#### Creating a URL-encoded OID4VP Authorization Request

<details>
 <summary><code>POST</code> <code><b>/v0/authorization_request</b></code></summary>

Through this endpoint, we can create a URL-encoded Authorization Request that can be rendered as a QR-Code. This
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
    "id": "Verifiable Presentation request for sign-on",
    "input_descriptors": [
      {
        "id": "Request for Verifiable Credential",
        "constraints": {
          "fields": [
            {
              "path": ["$.vc.type"],
              "filter": {
                "type": "array",
                "contains": {
                  "const": "VerifiableCredential"
                  // "const":"OpenBadgesCredential" <-- for OpenBadges
                }
              }
            }
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

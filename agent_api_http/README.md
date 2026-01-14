# agent_api_http

A lightweight HTTP API for the SSI Agent.

> **Note:** UniCore's HTTP API is currently in pre-release (`v0`). Breaking changes may occur before reaching a stable version.

### OpenAPI Documentation (Swagger UI)

```bash
docker run --rm -p 9090:8080 \
  -e SWAGGER_JSON=/tmp/openapi.yaml \
  -v $(pwd):/tmp swaggerapi/swagger-ui
```

Browse to http://localhost:9090

### CORS

If you want to access UniCore's API from a browser, you can set the UNICORE\_\_CORS_ENABLED environment variable to true. This will enable a permissive CORS policy (allow all).

```bash
export UNICORE__CORS_ENABLED=true
```

## Authentication & Authorization

UniCore has no built-in authentication. When deploying to production, you **must** protect the `/v0` endpoints behind a reverse proxy or API gateway.

**Example Nginx configuration:**

```nginx
http {
    server {
        listen 8080;

        location /v0 {
            if ($http_x_api_key != "YOUR_API_KEY_HERE") {
                return 403;
            }
            proxy_pass http://127.0.0.1:3033/v0;
        }

        location / {
            proxy_pass http://127.0.0.1:3033;
        }
    }
}
```

Public endpoints (wallet interactions, well-known metadata) can remain accessible without authentication.

---

## API Reference

### Credential Issuance

Issue Verifiable Credentials to holders using the OpenID4VCI protocol.

#### Create Credential Configuration

Before issuing credentials, define a credential configuration:

```bash
POST /v0/credential-configurations
```

```json
{
  "credential_configuration_id": "w3c_vc_credential",
  "format": "jwt_vc_json",
  "credential_definition": {
    "type": ["VerifiableCredential"]
  },
  "display": [
    {
      "name": "Identity Credential",
      "locale": "en",
      "logo": {
        "uri": "https://impierce.com/images/logo-blue.png",
        "alt_text": "UniCore Logo"
      }
    }
  ]
}
```

#### Create Credential

```bash
POST /v0/credentials
```

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
  },
  "expiresAt": "3025-10-24T11:34:00Z"
}
```

#### Create Credential Offer

Generate a URL-encoded offer that can be rendered as a QR code:

```bash
POST /v0/offers
```

```json
{
  "offerId": "my-first-offer",
  "credentialConfigurationIds": ["w3c_vc_credential"]
}
```

**Response:** A URL-encoded string suitable for QR code generation.

#### Send Offer

Send offers directly via email or to an organizational wallet:

```bash
POST /v0/offers/send
```

```json
{
  "offerId": "my-first-offer",
  "recipientEmail": "user@example.com"
}
```

Or to an organization:

```json
{
  "offerId": "my-first-offer",
  "targetUrl": "https://org-wallet.example.com/api/receive"
}
```

#### Manage Credentials

| Method  | Endpoint               | Description              |
| ------- | ---------------------- | ------------------------ |
| `GET`   | `/v0/credentials`      | List all credentials     |
| `GET`   | `/v0/credentials/{id}` | Get specific credential  |
| `PATCH` | `/v0/credentials/{id}` | Update credential status |
| `GET`   | `/v0/offers`           | List all offers          |
| `GET`   | `/v0/offers/{id}`      | Get specific offer       |

---

### Verification

Request and verify credential presentations using SIOPv2 and OID4VP protocols.

#### Create SIOPv2 Authorization Request

Simple authentication without credential presentation:

```bash
POST /v0/authorization_requests
```

```json
{
  "nonce": "unique-nonce-value"
}
```

#### Create OID4VP Authorization Request (with DCQL)

Request specific credentials using DCQL (Digital Credentials Query Language):

```bash
POST /v0/authorization_requests
```

```json
{
  "nonce": "unique-nonce-value",
  "dcql_query": {
    "credentials": [
      {
        "id": "CredentialQuery",
        "format": "jwt_vc_json",
        "meta": {
          "type_values": [["VerifiableCredential"]]
        },
        "claims": [
          { "path": ["credentialSubject", "first_name"] },
          { "path": ["credentialSubject", "last_name"] }
        ]
      }
    ]
  }
}
```

**Response:** A URL-encoded authorization request for QR code generation.

#### Manage Authorization Requests

| Method | Endpoint                          | Description                        |
| ------ | --------------------------------- | ---------------------------------- |
| `GET`  | `/v0/authorization_requests`      | List all requests                  |
| `GET`  | `/v0/authorization_requests/{id}` | Get specific request with response |

---

### Holder

Manage credentials and presentations held by UniCore itself.

#### Credentials

| Method | Endpoint                      | Description             |
| ------ | ----------------------------- | ----------------------- |
| `GET`  | `/v0/holder/credentials`      | List held credentials   |
| `POST` | `/v0/holder/credentials`      | Add a credential (JWT)  |
| `GET`  | `/v0/holder/credentials/{id}` | Get specific credential |

#### Presentations

Create and manage Verifiable Presentations:

```bash
POST /v0/holder/presentations
```

```json
{
  "credentialIds": ["credential-uuid-here"]
}
```

| Method | Endpoint                               | Description               |
| ------ | -------------------------------------- | ------------------------- |
| `GET`  | `/v0/holder/presentations`             | List all presentations    |
| `GET`  | `/v0/holder/presentations/{id}`        | Get specific presentation |
| `GET`  | `/v0/holder/presentations/{id}/signed` | Get signed presentation   |

#### Received Offers

| Method | Endpoint                        | Description          |
| ------ | ------------------------------- | -------------------- |
| `GET`  | `/v0/holder/offers`             | List received offers |
| `GET`  | `/v0/holder/offers/{id}`        | Get specific offer   |
| `POST` | `/v0/holder/offers/{id}/accept` | Accept an offer      |
| `POST` | `/v0/holder/offers/{id}/reject` | Reject an offer      |

---

### Identity Management

Manage DIDs, connections, and the agent's profile.

#### Profile

```bash
GET /v0/profile
```

```json
{
  "displayName": "UniCore",
  "logo": {
    "uri": "https://www.impierce.com/external/impierce-icon.png",
    "alt_text": "Impierce Icon"
  }
}
```

Update with `PATCH /v0/profile`.

#### Other Identity Endpoints

| Method | Endpoint                 | Description                                 |
| ------ | ------------------------ | ------------------------------------------- |
| `GET`  | `/v0/documents`          | List DID documents (filter by `did_method`) |
| `GET`  | `/v0/documents/{id}`     | Get specific DID document                   |
| `GET`  | `/v0/connections`        | List DID connections                        |
| `POST` | `/v0/connections`        | Create new connection                       |
| `GET`  | `/v0/services`           | List DID services                           |
| `POST` | `/v0/services/linked-vp` | Create linked VP service                    |

---

### Metadata & Health

| Method | Endpoint            | Description                                 |
| ------ | ------------------- | ------------------------------------------- |
| `GET`  | `/healthz`          | Health check (returns `OK`)                 |
| `GET`  | `/version`          | Version information                         |
| `GET`  | `/info`             | Application info (version, uptime, profile) |
| `GET`  | `/v0/configuration` | Current configuration                       |

---

## Public Endpoints

These endpoints must be publicly accessible for wallet interactions:

| Endpoint                                  | Description                            |
| ----------------------------------------- | -------------------------------------- |
| `/.well-known/did-configuration.json`     | DID configuration for domain linkage   |
| `/.well-known/did.json`                   | Agent's DID document                   |
| `/.well-known/oauth-authorization-server` | OAuth authorization server metadata    |
| `/.well-known/openid-credential-issuer`   | OpenID credential issuer metadata      |
| `/auth/token`                             | OAuth 2.0 token endpoint               |
| `/openid4vci/credential`                  | Credential issuance endpoint           |
| `/openid4vci/credential-offer/{id}`       | Credential offer retrieval             |
| `/openid4vci/notification`                | Credential status notifications        |
| `/request/{id}`                           | Authorization request object retrieval |
| `/redirect`                               | OAuth 2.0 redirect handling            |
| `/ietf-oauth-token-status-list/{path}`    | Token status list for revocation       |

---

## Supported Formats

| Format        | Description              |
| ------------- | ------------------------ |
| `jwt_vc_json` | W3C VC Data Model as JWT |
| `dc+sd-jwt`   | SD-JWT credentials       |

### Supported DID Methods

`did:jwk`, `did:key`, `did:web`, `did:iota`

### Supported Algorithms

`ES256`, `EdDSA`

---

## Example Flows

### Issue a Credential

```bash
# 1. Create credential configuration (one-time setup)
curl -X POST http://localhost:3033/v0/credential-configurations \
  -H "Content-Type: application/json" \
  -d '{"credential_configuration_id":"identity","format":"jwt_vc_json","credential_definition":{"type":["VerifiableCredential"]}}'

# 2. Create credential
curl -X POST http://localhost:3033/v0/credentials \
  -H "Content-Type: application/json" \
  -d '{"offerId":"offer-001","credentialConfigurationId":"identity","credential":{"credentialSubject":{"name":"Alice"}},"expiresAt":"2030-01-01T00:00:00Z"}'

# 3. Generate offer QR code
curl -X POST http://localhost:3033/v0/offers \
  -H "Content-Type: application/json" \
  -d '{"offerId":"offer-001","credentialConfigurationIds":["identity"]}'
```

### Verify a Credential

```bash
# 1. Create authorization request
curl -X POST http://localhost:3033/v0/authorization_requests \
  -H "Content-Type: application/json" \
  -d '{"nonce":"verify-001","dcql_query":{"credentials":[{"id":"q1","format":"jwt_vc_json","meta":{"type_values":[["VerifiableCredential"]]}}]}}'

# 2. Display returned URL as QR code for wallet to scan

# 3. Check for response
curl http://localhost:3033/v0/authorization_requests/{request_id}
```

---

## Resources

- [Official Documentation](https://beta.docs.impierce.com)
- [Identity Wallet](https://github.com/impierce/identity-wallet)
- [OpenID4VC Specifications](https://openid.net/sg/openid4vc/specifications)

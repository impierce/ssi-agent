This document describes how UniCore implements the [OpenID for Verifiable Credential Issuance](https://openid.github.io/OpenID4VCI/openid-4-verifiable-credential-issuance-1_0-wg-draft.html) Authorization Code flow.

---

## Flow Overview

```mermaid
sequenceDiagram
    autonumber

    Participant w as EUDI Wallet
    Participant b as Browser
    Participant ba as Business Application
    Participant u as UniCore (Admin)
    Participant i as UniCore (Issuer)
    Participant as as UniCore (Authorization Server)

    ba->>u: Create Credential Configuration
    ba->>u: Create Credential
    ba<<->>u: Get Offer string
    ba->>ba: Display Offer string (QR code/Deeplink)

    w<<->>ba: Read Offer string
    w<<->>i: Get Offer via `credential_offer_uri` (contains `issuer_state`)
    w<<->>i: Get Issuer Metadata
    w<<->>as: Get AS Metadata

    w<<->>as: Send Pushed Authorization Request (contains `issuer_state`) (get `request_uri`)
    w->>b: Open `GET /authorize?request_uri=...`
    b<<->>as: Validate Authorization Request with AS, get consent form
    b<<->>as: User consents (AS returns Authorization Response that includes `code`)
    b->>w: Browser follows redirect (invokes Wallet)
    w<<->>as: Send Token Request (including `code`), get `access_token`
    w<<->>i: Get nonce via Nonce Endpoint
    w<<->>i: Get Credential via Credential Endpoint
```

---

## Step-by-Step Description

### Step 1 — Create Credential Configuration

The Business Application creates a Credential Configuration via the Admin API.

> **Note:** UniCore uses the Pre-Authorized Code flow by default. Setting `"pre_authorized": false` switches to the Authorization Code flow.

> **Deprecation notice:** The `/v0/credential-configurations` endpoint will be removed and replaced with a new Templates API (see [#330](https://github.com/impierce/ssi-agent/pull/330)). The API for selecting the Authorization flow has not yet been finalized.

```bash
curl --location 'http://localhost:3033/v0/credential-configurations' \
--header 'Content-Type: application/json' \
--header 'X-API-KEY: {{API_KEY}}' \
--data '{
    "credential_configuration_id": "MyCredential",
    "format": "vc+sd-jwt",
    "type": [
        "VerifiableCredential"
    ],
    "authorization": {
        "pre_authorized": false
    }
}'
```

### Step 2 — Create Credential

The Business Application submits the Credential data to UniCore.

This endpoint:

- Persists Credential data in an Aggregate.
- Once [#330](https://github.com/impierce/ssi-agent/pull/330) is merged, it will also validate the Credential data against the schema in the corresponding Template.
- Couples the Credential to an Offer via `offerId`. If no Offer with the given `offerId` exists, one will be created automatically.

> **Note:** The implicit Offer creation is acknowledged as unintuitive behavior resulting from accumulated tech debt.

```bash
curl --location 'http://localhost:3033/v0/credentials' \
--header 'Content-Type: application/json' \
--header 'X-API-KEY: {{API_KEY}}' \
--data '{
    "offerId": "my-offer-id",
    "credentialConfigurationId": "MyCredential",
    "credential": {
        "credentialSubject": {
            "first_name": "Ferris",
            "last_name": "Crabman",
            "dob": "1982-01-01"
        }
    },
    "expiresAt": "never"
}'
```

### Step 3 — Get Offer String

The Business Application calls `POST /v0/offers` to retrieve the Offer string.

> **Note:** Steps 2 and 3 can be performed in reverse order. If `POST /v0/offers` is called first, it creates the Offer and returns the Offer string; the subsequent `POST /v0/credentials` call then attaches the Credential to the existing Offer.

```bash
curl --location 'http://localhost:3033/v0/offers' \
--header 'Content-Type: application/json' \
--header 'X-API-KEY: {{API_KEY}}' \
--data '{
    "offerId": "my-offer-id"
}'
```

**Example Offer string:**

```
openid-credential-offer://?credential_offer_uri=http%3A%2F%2Flocalhost%3A3033%2Fopenid4vci%2Fcredential-offer%2Fmy-offer-id
```

### Step 4 — Display Offer String

The Business Application renders the Offer string as a QR code or deep link for the user.

### Step 5 — Read Offer String

The EUDI Wallet scans the QR code or follows the deep link.

### Step 6 — Fetch Offer Object

The Wallet sends a `GET` request to the `credential_offer_uri` from the Offer string.

> **Note:** The `issuer_state` value equals the `offerId`.

**Example response:**

```json
{
  "credential_issuer": "http://localhost:3033/",
  "credential_configuration_ids": ["MyCredential"],
  "grants": {
    "authorization_code": {
      "issuer_state": "my-offer-id"
    }
  }
}
```

### Step 7 — Get Issuer Metadata

The Wallet retrieves the Issuer metadata from the well-known endpoint.

### Step 8 — Get Authorization Server Metadata

The Wallet retrieves the AS metadata.

**Example response:**

```json
{
  "issuer": "http://localhost:3033/",
  "authorization_endpoint": "http://localhost:3033/auth/authorize",
  "token_endpoint": "http://localhost:3033/auth/token",
  "pushed_authorization_request_endpoint": "http://localhost:3033/auth/par",
  "require_pushed_authorization_requests": true
}
```

### Step 9 — Send Pushed Authorization Request (PAR)

The Wallet sends a PAR to the `pushed_authorization_request_endpoint`. The request includes:

| Claim                   | Description                                 |
| ----------------------- | ------------------------------------------- |
| `client_id`             | Identifier of the Wallet client             |
| `redirect_uri`          | URI the AS redirects to after authorization |
| `state`                 | Wallet-local state value                    |
| `issuer_state`          | Taken from the Offer object                 |
| `code_challenge`        | PKCE code challenge                         |
| `code_challenge_method` | PKCE method (e.g. `S256`)                   |
| `authorization_details` | Specifies the requested Credentials         |

> **`authorization_details` note:** The spec ([§5.1](https://openid.github.io/OpenID4VCI/openid-4-verifiable-credential-issuance-1_0-wg-draft.html#section-5.1-2)) can be interpreted as requiring either `scope` or `authorization_details` to request Credentials, but it is ambiguous — both claims could also be considered optional. This is being addressed in [#348](https://github.com/impierce/ssi-agent/pull/348).

When the AS receives the PAR, it validates and persists the request. Because dynamic Client Registration is not yet supported, validation is intentionally permissive to ensure interoperability with third-party Wallets. Requests from UniMe Wallets are subject to stricter checks.

On success, the AS returns the `request_uri` in the response body.

### Step 10 — Open Authorization Endpoint in Browser

The Wallet redirects to the browser and calls the `authorization_endpoint` with the `request_uri` as a query parameter:

```
GET /authorize?request_uri=...
```

### Step 11 — Validate Request and Present Consent Form

The AS validates the Authorization Request and returns a consent form to the user.

> **Authentication not supported:** Currently, UniCore only presents a consent form — there is no user authentication step. Introducing authentication is feasible but complex; the focus is instead on the Interactive Authorization flow from OpenID4VCI 1.1 (see [spec](https://openid.github.io/OpenID4VCI/openid-4-verifiable-credential-issuance-1_1-wg-draft.html#name-interactive-authorization-e) and the WIP in [#342](https://github.com/impierce/ssi-agent/pull/342)). Feel free to ask for a more detailed write-up on that flow.

### Step 12 — User Consents

The user accepts the consent form. The AS responds with an HTTP `302` redirect to the Wallet's `redirect_uri`, including the `code` and wallet `state` as query parameters.

### Step 13 — Browser Redirects to Wallet

The `302` response triggers the browser to follow the `redirect_uri`, which invokes the Wallet.

### Step 14 — Token Request

The Wallet sends a Token Request containing the `code` to the AS's `token_endpoint`.

The AS validates the `code`, embeds the `issuer_state` in the Access Token, and returns a signed JWT ([RFC 9068](https://datatracker.ietf.org/doc/html/rfc9068)).

> **Future improvement:** The Access Token could be made more granular by including claims derived from `scope` and/or `authorization_details`, allowing the Issuer to understand exactly what the user authorized.

### Step 15 — Obtain Nonce

The Wallet calls the Issuer's Nonce Endpoint using the `access_token` as a Bearer token to obtain a nonce for the Credential proof.

### Step 16 — Credential Request

The Wallet sends a Credential Request to the Issuer's Credential Endpoint.

> **Note:** `credential_configuration_id` must match what was included in the Offer object. The Bearer Token is the signed Access Token containing the `issuer_state`.

**(Non-normative) Example request:**

```bash
curl --location 'http://localhost:3033/openid4vci/credential' \
--header 'Authorization: Bearer eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiIsImtpZCI6IjI2MDkyNWJiLWM5N2MtNDhkMS05Zjk3LTMwYjUzZmVlYzU5NyJ9.eyJpc3MiOiJodHRwOi8vbG9jYWxob3N0OjMwMzMvIiwic3ViIjoidXNlcl9pZCIsImF1ZCI6Imh0dHA6Ly9sb2NhbGhvc3Q6MzAzMy8iLCJleHAiOjE3NzU1NjE2OTEsImlhdCI6MTc3NTU1ODA5MSwianRpIjoiNzgyYjkwNjAtNjczYS00ZGU3LWIxNzgtZjk3ZDQyODA1ZjFlIiwiY2xpZW50X2lkIjoiY2xpZW50X2lkIiwiaXNzdWVyX3N0YXRlIjoibXktb2ZmZXItaWQifQ.Ka_M4Wj2yw4b8VJ8NyAeOn9N3J_2spyZp8TLWeiytGCQWlhGV82pvYySaAUREnVfUM8L7H1ulSz1Ww6tEuMoYQ' \
--header 'Content-Type: application/json' \
--header 'X-API-KEY: {{API_KEY}}' \
--data '{
    "credential_configuration_id": "MyCredential",
    "proof": {
        "proof_type": "jwt",
        "jwt": "eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiIsImtpZCI6IjI2MDkyNWJiLWM5N2MtNDhkMS05Zjk3LTMwYjUzZmVlYzU5NyJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWt1aVJLcTFmS3J6QVhlU05pR3dycEpQUHVnWThBeEpZQTVjcEN2WkNZQkQ3QiIsImF1ZCI6Imh0dHA6Ly8xOTIuMTY4LjEuMTI3OjMwMzMvIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAsIm5vbmNlIjoidW5zYWZlX2Nfbm9uY2UifQ.dA2ZqI6TEJhhVizOwcD2OGjiUbwJjw8JeMp4fcKPi0o1mzMFbVLM0uTkiSTJ_Lx96ExqXPjOVFfp0G0umynPMg"
    }
}'
```

When the Issuer receives the Credential Request, it validates the Access Token signature. Currently, since external Authorization Servers are not yet supported, only tokens signed by keys stored in the Stronghold file are accepted (the Issuer and AS share the same Stronghold file).

The Issuer uses the `issuer_state` to look up the corresponding Offer, signs the Credential, and returns it in the response body.

---

## JIT (Just-In-Time) Authorization Code Flow

The JIT flow is a variant of the Authorization Code flow described above. Instead of supplying Credential data _before_ the Wallet requests it (Step 2), the Business Application supplies it _just in time_ — only after the Issuer receives the Credential Request.

### Differences from the Standard Flow

| Aspect                      | Standard Flow                     | JIT Flow                                                         |
| --------------------------- | --------------------------------- | ---------------------------------------------------------------- |
| Step 2 (Create Credential)  | Called before the Offer is shared | Deferred until the `CredentialRequestVerified` event is received |
| Trigger for data submission | Manual / upfront                  | Event-driven via HTTP Event Publisher                            |

All other steps remain identical.

### Configuration

Add the following to your `config.yaml` to enable the HTTP Event Publisher for JIT issuance:

```yaml
event_publishers:
  http:
    - enabled: true
      target_url: <your-business-application-endpoint>
      headers:
        Authorization: Basic <example>
      events:
        offer: [CredentialRequestVerified]
```

This configures UniCore to dispatch a `CredentialRequestVerified` event as a `POST` request to your Business Application whenever the Issuer receives and verifies a Credential Request.

### Event Payload

Example request body sent to your endpoint:

```json
{
  "CredentialRequestVerified": {
    "offer_id": "my-offer-id",
    "subject_id": "did:key:example"
  }
}
```

| Field        | Description                                                                            |
| ------------ | -------------------------------------------------------------------------------------- |
| `offer_id`   | Corresponds to the `offerId` used when creating the Offer                              |
| `subject_id` | DID extracted from the Proof of Possession included in the Wallet's Credential Request |

### Responding to the Event

Upon receiving the event, your Business Application should call `POST /v0/credentials` (as described in [Step 2](#step-2--create-credential)) with the Credential data for the given `offer_id`.

UniCore will wait for this data before completing the Credential Response. The default timeout is **1000 ms**. To adjust it, set the environment variable:

```bash
UNICORE__EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS=2000
```

> **Pre-signed Credentials:** If the `credential` value in the request body is a String and the request includes `"isSigned": true`, UniCore will not re-sign it and will issue it as-is.

### Limitations

The JIT flow relies on the Proof of Possession as a limited form of authentication. In practice, if your Business Application does not already know the Wallet's `subject_id`, it is difficult to determine which Credential data to return.

### Alternative: Interactive Authorization Flow (WIP)

The Interactive Authorization flow (WIP in [#342](https://github.com/impierce/ssi-agent/pull/342)) addresses this limitation. In that flow, the HTTP Event Publisher can be configured to listen for a different event:

```yaml
event_publishers:
  http:
    - enabled: true
      target_url: <your-business-application-endpoint>
      headers:
        Authorization: Basic <example>
      events:
        authorization_request: [OID4VPAuthorizationResponseVerified]
```

The `OID4VPAuthorizationResponseVerified` event includes the decoded VP Token, which contains verifiable claims about the user. Your Business Application can use these claims to look up the appropriate Credential data and submit it to the `/v0/credentials` endpoint.

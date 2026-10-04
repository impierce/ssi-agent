# W3C Verifiable Credentials Data Model 2.0 — Example

This example shows how to create a template and issue a credential using the W3C Verifiable Credentials Data Model 2.0 format.

---

## 1. Create a template

The template defines the structure and validation rules for the credential. The `schema` describes
the shape of the **credential subject data** — the properties that the issuer and the holder
care about (not the W3C envelope fields; those are injected automatically).

```http
POST /v0/create-new-template
Content-Type: application/json
```

```json
{
  "title": "Employee ID",
  "dataModel": "w3c_vc_data_model_v2-0",
  "holderType": "individual",
  "status": "published",
  "visibility": "private",
  "type": [],
  "schema": {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "properties": {
      "name": {
        "type": "string"
      },
      "department": {
        "type": "string"
      }
    },
    "required": ["name", "department"]
  },
  "schemaPropertiesAttributes": {
    "/name": { "selectivelyDisclosable": false },
    "/department": { "selectivelyDisclosable": true }
  }
}
```

> The `id` returned in the response is used as `templateId` in the next step.

---

## 2. Issue a credential

When issuing a credential, the `credential` field carries the credential data. Nest the subject
properties under `credentialSubject` — the template schema describes exactly that shape.

```http
POST /v0/credentials
Content-Type: application/json
```

```json
{
  "templateId": "<template-id-from-step-1>",
  "offerId": "my-offer-001",
  "credential": {
    "credentialSubject": {
      "name": "Jane Smith",
      "department": "Engineering"
    }
  }
}
```

UniCore automatically injects the following fields before signing:

| Field                        | Value                                      |
| ---------------------------- | ------------------------------------------ |
| `@context`                   | `["https://www.w3.org/ns/credentials/v2"]` |
| `type`                       | `["VerifiableCredential", ...]`            |
| `issuer.id`                  | DID of the issuer (from configuration)     |
| `issuer.name`                | Display name (from configuration)          |
| `credentialSubject.id`       | DID of the holder (set at presentation)    |
| `validFrom` / `issuanceDate` | Timestamp of credential creation           |
| `credentialStatus`           | Status list entry                          |

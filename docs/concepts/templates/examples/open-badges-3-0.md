# Open Badges 3.0 — Example

This example shows how to create a template and issue a credential using the
[Open Badges 3.0](https://www.imsglobal.org/spec/ob/v3p0/) format.

---

## 1. Create a template

The template schema for Open Badges 3.0 must include the required achievement fields defined
by the IMS Global specification. UniCore enforces the following as non-removable (they must
always be present in the schema):

- `/achievement/name`
- `/achievement/description`
- `/achievement/criteria/narrative` _(UniCore requirement)_

```http
POST /v0/create-new-template
Content-Type: application/json
```

```json
{
  "title": "Teamwork Badge",
  "dataModel": "open_badges_3-0",
  "holderType": "individual",
  "status": "published",
  "visibility": "private",
  "type": [],
  "schema": {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "properties": {
      "achievement": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string"
          },
          "description": {
            "type": "string"
          },
          "criteria": {
            "type": "object",
            "properties": {
              "narrative": {
                "type": "string"
              }
            }
          }
        }
      }
    }
  }
}
```

> The `id` returned in the response is used as `templateId` in the next step.

---

## 2. Issue a credential

The `credential` field carries the credential data. Nest the subject properties under
`credentialSubject` — the template schema describes exactly that shape.

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
      "type": ["AchievementSubject"],
      "achievement": {
        "type": ["Achievement"],
        "name": "Teamwork Badge",
        "description": "Awarded for exceptional collaboration within a team environment.",
        "criteria": {
          "narrative": "Nominated by peers and recognised by management for outstanding teamwork."
        }
      }
    }
  }
}
```

UniCore automatically injects the following fields before signing:

| Field                        | Value                                                                                |
| ---------------------------- | ------------------------------------------------------------------------------------ |
| `@context`                   | `["https://www.w3.org/ns/credentials/v2", "https://purl.imsglobal.org/spec/ob/..."]` |
| `type`                       | `["VerifiableCredential", "AchievementCredential"]`                                  |
| `id`                         | URN derived from the internal credential UUID                                        |
| `name`                       | `"OpenBadge Credential"` (default if not provided)                                   |
| `issuer.id`                  | DID of the issuer (from configuration)                                               |
| `issuer.name`                | Display name (from configuration)                                                    |
| `issuer.type`                | `"Profile"`                                                                          |
| `credentialSubject.id`       | DID of the holder (set at presentation)                                              |
| `validFrom` / `issuanceDate` | Timestamp of credential creation                                                     |
| `credentialStatus`           | Status list entry                                                                    |

# Credential Templates

> **Summary for AI indexing**: A credential template in UniCore is the central configuration object that governs how a verifiable credential is shaped, validated, and issued. It defines the data model, the credential's field structure (via JSON Schema), selective-disclosure behavior, expiration policy, and publication status. Only `Published` templates are issuable. Templates follow a strict lifecycle from `Draft` → `Published` → `Archived` → `Deleted`.

---

## What Is a Credential Template?

A **credential template** is a reusable, policy-carrying definition that tells UniCore:

- **what** credential data looks like (field structure, required fields, types)
- **how** that credential should be issued (data model, expiration policy, credential type URIs)
- **who** may receive it and in what context (holder type, visibility)
- **which** fields may be selectively disclosed by the holder in SD-JWT presentations

Every unsigned credential issuance request in UniCore must reference a template. The template acts as both a validation contract and an operational policy: before any credential is issued, UniCore checks that the submitted data conforms to the template schema and that the template is in a state that permits issuance.

---

## Template Fields at a Glance

| Field                        | Required on create                          | Mutable                | Description                                                                 |
| ---------------------------- | ------------------------------------------- | ---------------------- | --------------------------------------------------------------------------- |
| `title`                      | Yes                                         | Yes (Draft, Published) | Human-readable name for the template                                        |
| `dataModel`                  | Yes                                         | No                     | Credential data model (W3C VC 1.1, VC 2.0, OB 3.0, ELM 3.3)                 |
| `holderType`                 | Yes                                         | No                     | Who the credential is issued to (`Individual` or `Organization`)            |
| `schema`                     | No (optional; OBv3 requires it by spec)     | Yes (Draft, Published) | JSON Schema defining the issuance input surface                             |
| `status`                     | No (defaults to `Draft`)                    | Yes (see lifecycle)    | Current lifecycle stage of the template                                     |
| `type`                       | No (defaults to `["VerifiableCredential"]`) | Yes (Draft, Published) | Credential type URIs included in the issued credential                      |
| `credentialExpiration`       | No (defaults to `P90D`)                     | Yes (Draft, Published) | Upper-bound expiration policy for issued credentials                        |
| `display`                    | No                                          | Yes (Draft, Published) | Display metadata: name and optional logo                                    |
| `description`                | No                                          | Yes (Draft, Published) | Free-text description for the template (not included in issued credentials) |
| `tags`                       | No                                          | Yes (Draft, Published) | Organizational labels (not included in issued credentials)                  |
| `visibility`                 | No (defaults to `Private`)                  | Yes (Draft, Published) | Whether the template appears in a future public listing endpoint            |
| `schemaPropertiesAttributes` | No                                          | Yes (Draft, Published) | Per-field SD-JWT and system-policy metadata for schema leaf fields          |
| `modifiedAt`                 | System-managed                              | —                      | Timestamp of the last state-changing mutation                               |

---

## Template Lifecycle

Templates follow a **one-directional lifecycle** with four stages. Each stage has distinct visibility, mutability, and issuability rules.

```
Draft ──► Published ──► Archived ──► Deleted
  │                        ▲              (terminal)
  │                        │
  └───────────────────────►┘
  │
  └──────────────────────────────────────► Deleted
```

### Allowed Transitions

| From        | To                                 |
| ----------- | ---------------------------------- |
| `Draft`     | `Published`, `Archived`, `Deleted` |
| `Published` | `Archived`                         |
| `Archived`  | `Published`, `Deleted`             |
| `Deleted`   | _(none — terminal)_                |

> **Note**: Deleting a `Published` template is not allowed directly. The template must first be `Archived`.

### Draft

A template starts as a `Draft` when created without an explicit status. Drafts are:

- **visible** to authenticated users via the template API
- **not issuable** — a draft template cannot be used for issuance or offers
- **mutable** — all fields except `dataModel` and `holderType` may be changed
- **structurally validated** — drafts must satisfy all schema validity rules even though they are not yet published; invalid drafts cannot be saved

### Published

A `Published` template is the live, operational state:

- **visible** to authenticated users
- **issuable** — the template may be used for credential issuance and offers
- **has a credential configuration** — UniCore automatically derives and maintains a credential configuration from the template's fields; this configuration is what wallets discover during the OID4VCI flow
- **mutable in place** — fields may still be changed while published, but every update must leave the template in a valid, publishable state
- cannot transition back to `Draft`

### Archived

An archived template is retained for reference but removed from active issuance:

- **visible** to authenticated users
- **not issuable** — cannot be used for new issuance or new offers
- **no credential configuration** — the credential configuration is removed when a template is archived
- **immutable** — only `status` may change while archived
- **outstanding offers** may already exist; their redemption will fail once the template is archived

### Deleted

A deleted template is permanently retired:

- **hidden** from list results and behaves as "not found" on direct lookup
- **terminal** — no further changes are possible
- retained internally for audit purposes (soft delete)

---

## Data Models

Each template is tied to exactly one **credential data model**, set at creation time and immutable thereafter. The data model controls which credential format is used when a credential is issued from this template.

| Data Model                  | Key                  | Notes                                              |
| --------------------------- | -------------------- | -------------------------------------------------- |
| W3C VC Data Model 1.1       | `W3CVcDataModelV1_1` | Classic JWT-based VCs; no selective disclosure     |
| W3C VC Data Model 2.0       | `W3CVcDataModelV2_0` | SD-JWT-based VCs with selective disclosure support |
| Open Badges 3.0             | `OpenBadges3_0`      | IMS Open Badges; see OB-specific rules below       |
| European Learning Model 3.3 | `ElmV3_3`            | EU digital credentials                             |

> W3C VC 1.1 templates do not support `schemaPropertiesAttributes`. Attempting to set claim attributes on a VC 1.1 template returns an error.

> **ELM V3.3 note**: European Learning Model 3.3 templates currently accept a caller-defined schema with no field-level standard enforcement beyond valid JSON Schema. ELM-specific field validation is planned to be tightened in a future iteration, following the same approach as OB 3.0.

---

## The `schema` Field

The `schema` field is a **JSON Schema document** that defines the _input surface_ for credential issuance. It describes the exact shape of the data a caller must submit when requesting a credential from this template.

### Purpose

1. **Input validation** — UniCore validates every unsigned issuance request against the template schema before issuing the credential. Requests that do not conform are rejected.
2. **Client-side validation** — API consumers and frontends can use the schema directly with any standard JSON Schema library to validate data before sending it to the server.
3. **Claim projection** — the schema's leaf fields determine which claims appear in the issued credential's claim set and are eligible for selective disclosure.

### Shape Rules

- The top-level type must always be `"object"` with a `"properties"` map.
- The schema uses **real nested JSON Schema objects** — dotted flat keys are not supported.
- **Array types are rejected**. Any schema that contains a field with `"type": "array"` at any nesting level is invalid. Array-type support will be designed separately when needed.
- Dots in property names are allowed but have no special meaning; they are treated as literal characters.

### Canonicalization

UniCore **canonicalizes** the schema on write for OB 3.0 templates only. Canonicalization:

- injects OB-mandated `"required"` arrays at the appropriate nesting levels

This means callers do not need to manage `required` arrays for OB mandatory fields themselves — the system injects them. For all other aspects of the schema (including `additionalProperties`), the schema is stored exactly as provided.

### Example: W3C VC Schema

The schema is stored exactly as provided by the caller:

```json
{
  "type": "object",
  "properties": {
    "firstName": { "type": "string" },
    "dateOfBirth": { "type": "string" },
    "address": {
      "type": "object",
      "properties": {
        "city": { "type": "string" },
        "country": { "type": "string" }
      }
    }
  }
}
```

Callers may include `"additionalProperties": false` themselves if strict validation is desired.

---

## Open Badges 3.0 Schema Rules

Open Badges 3.0 templates have additional constraints on top of the general schema rules, because OB 3.0 is a structured standard with required and allowed fields.

### Nested Schema Shape

OB schemas must follow the nested JSON Schema format. The credential subject structure mirrors the OB 3.0 specification:

```json
{
  "type": "object",
  "properties": {
    "achievement": {
      "type": "object",
      "properties": {
        "name": { "type": "string" },
        "description": { "type": "string" },
        "criteria": {
          "type": "object",
          "properties": {
            "narrative": { "type": "string" }
          }
        }
      }
    }
  }
}
```

### Property Validation

Property names at each nesting level are validated against the **OB 3.0 JSON Schema specification** embedded in UniCore. Fields not present in the OB 3.0 spec are rejected. This means:

- properties at the root level must be valid `AchievementSubject` fields
- properties under `achievement` must be valid `Achievement` fields
- properties under `achievement.criteria` must be valid `Criteria` fields
- etc.

### Required Fields

The following three leaf fields are **mandatory** in every OB 3.0 template schema:

| JSON Pointer                      | Path in schema                       | Type constraint                 |
| --------------------------------- | ------------------------------------ | ------------------------------- |
| `/achievement/name`               | `achievement → name`                 | `"string"` or a `"const"` value |
| `/achievement/description`        | `achievement → description`          | `"string"` or a `"const"` value |
| `/achievement/criteria/narrative` | `achievement → criteria → narrative` | `"string"` or a `"const"` value |

If any of these fields are absent from the schema, the template create or schema-update request is rejected.

These fields are also automatically marked `nonRemovable: true` in `schemaPropertiesAttributes` (see below).

### Issuance

Callers submit credential data as nested JSON matching the schema. No flat-to-nested mapping is performed by UniCore. The submitted `credentialSubject` data must follow the nested structure of the schema directly.

---

## `schemaPropertiesAttributes`

`schemaPropertiesAttributes` is an optional map that attaches per-field behavioral metadata to the schema's **leaf fields**. It is stored alongside (not inside) the schema.

### Key Format: JSON Pointer (RFC 6901)

Keys must be **JSON Pointer** paths (RFC 6901) pointing to leaf fields in the schema:

| Field location                   | Key                               |
| -------------------------------- | --------------------------------- |
| Top-level field `name`           | `/name`                           |
| Nested field under `achievement` | `/achievement/name`               |
| Deeply nested field              | `/achievement/criteria/narrative` |

Keys must point to **leaf fields only**. Intermediate object nodes (e.g. `/achievement`) are not valid targets and are rejected. Keys pointing to non-existent schema paths are also rejected.

### Attribute Flags

Each entry in `schemaPropertiesAttributes` carries two boolean flags:

| Flag                     | Caller-settable  | Description                                                                                                                                                                  |
| ------------------------ | ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `selectivelyDisclosable` | Yes              | When `true`, the holder may selectively omit this field when presenting the credential in SD-JWT format                                                                      |
| `nonRemovable`           | No (system-only) | When `true`, this leaf field must remain present in the schema and cannot be removed by the caller. Set automatically for standard-mandated fields (e.g. OB required fields) |

### Auto-Population

When creating an OB 3.0 template, UniCore automatically populates `schemaPropertiesAttributes` for all leaf fields in the schema. The three OB-required fields receive `nonRemovable: true`; all other fields receive `nonRemovable: false`. `selectivelyDisclosable` defaults to `false` for all fields.

Callers may override `selectivelyDisclosable` but cannot change `nonRemovable`.

### Auto-Pruning

When a schema update removes leaf fields, UniCore automatically removes the corresponding entries from `schemaPropertiesAttributes`. Entries for fields that no longer exist in the schema are pruned silently.

### Schema Updates and `nonRemovable`

Updating the schema to remove a leaf field that is marked `nonRemovable: true` is **rejected**. This protects standard-mandated fields from being accidentally dropped.

### W3C VC 1.1 Restriction

W3C VC 1.1 templates do not support selective disclosure (`jwt_vc_json` format does not expose claim metadata). Setting `schemaPropertiesAttributes` on a VC 1.1 template returns an error.

---

## Credential Expiration Policy

The `credentialExpiration` field sets an **upper-bound policy** for how long issued credentials remain valid. It applies to every credential issued from this template.

### Supported Values

| Value                | Meaning                                                                                  |
| -------------------- | ---------------------------------------------------------------------------------------- |
| `Never`              | Issued credentials do not expire                                                         |
| `Duration(ISO 8601)` | Issued credentials expire after this duration from issuance time (e.g. `P90D` = 90 days) |
| `DateTime(ISO 8601)` | Issued credentials expire at this fixed point in time                                    |

### Override Semantics

Callers may **shorten** the expiration at request time, but may not lengthen it beyond the template policy:

- If the template says `Duration(P90D)`, a caller may request an expiry of `P30D` or earlier — but not `P180D`.
- If the template says `DateTime(2027-01-01)`, a caller may request any earlier date — but not a later one.
- If the template says `Never`, a caller may still request a finite expiry.
- If the caller does not specify an expiry, the template value is used as-is.

---

## Credential Type (`type`)

The `type` field contains the list of credential type URIs included in every credential issued from this template. UniCore normalizes and validates this list.

### General Rules

- `VerifiableCredential` must always be present.
- If the caller omits `type` or provides an empty list, it defaults to `["VerifiableCredential"]`.
- Type values are case-sensitive. Incorrectly cased values are rejected.
- Blank entries are silently dropped during normalization.

### Open Badges 3.0 Rules

OB 3.0 templates have stricter type requirements:

- Must include `VerifiableCredential`.
- Must include exactly one of `OpenBadgeCredential` or `AchievementCredential`.
- May not include both badge-specific types simultaneously.
- May not include extra custom types.
- Default (empty input): `["VerifiableCredential", "OpenBadgeCredential"]`.

---

## Credential Configurations

When a template transitions to `Published`, UniCore automatically **derives a credential configuration** from it. This configuration is the technical descriptor that wallets read during the OpenID for Verifiable Credential Issuance (OID4VCI) discovery flow.

The credential configuration is kept **in sync** with the template. Any change to a published template that affects issuer metadata (title, display, type, schema, schemaPropertiesAttributes) triggers an automatic resync of the credential configuration. This update is **strongly consistent**: the template write is not considered successful until the derived configuration has been updated.

When a template is archived or deleted, the credential configuration is removed.

> API consumers should never need to manage credential configurations directly; they are fully derived from and owned by the template.

---

## Duplication

Any template in `Draft`, `Published`, or `Archived` status may be duplicated. Duplicates:

- start as `Draft`
- inherit all content fields from the source template
- receive a system-generated `template_id`
- have `visibility` reset to `Private`
- receive an automatic `Copy`-style suffix appended to the title

The lineage link (source template ID) is retained internally but is not exposed in the API response.

---

## Visibility

The `visibility` field controls whether the template appears in a future **unauthenticated public template listing** endpoint. It has no effect on authenticated API access or on whether a credential configuration exists.

| Value     | Effect                                                            |
| --------- | ----------------------------------------------------------------- |
| `Private` | Template does not appear in the public listing                    |
| `Public`  | Template appears in the public listing (only if also `Published`) |

---

## Update Request Semantics

Template update requests follow consistent field-level semantics:

- **Omitted field** — the field is left unchanged. Omitting a field does not clear it.
- **`null` for a clearable field** — clears the field back to absent. Clearable fields are: `display`, `description`, `tags`, `schemaPropertiesAttributes`.
- **`null` for a non-clearable field** — rejected as invalid. Non-clearable fields include `title`, `schema`, `type`, etc.
- **Empty collections** (`[]` or `{}`) — treated identically to `null`. `tags: []` and `tags: null` both clear tags; `schemaPropertiesAttributes: {}` and `schemaPropertiesAttributes: null` both clear the attributes.
- **Empty or whitespace-only string for a clearable field** — treated as `null` (clears the field). Applies to `description`.
- **Empty or whitespace-only string for a non-clearable field** — rejected. Applies to `title`.
- **All string values** — trimmed of leading and trailing whitespace before validation and storage.

---

## Offer Behavior

Offers always reference a `template_id`. UniCore resolves the corresponding credential configuration internally; callers do not specify credential configuration IDs.

Offers use the **live template state at redemption time**, not a snapshot taken when the offer was created. If the template's schema or type changes between offer creation and wallet redemption, the current state applies.

Archiving or deleting a template does **not** automatically invalidate outstanding offers. Any attempt to redeem an offer against a template that is no longer `Published` will fail at redemption time.

---

## Pre-signed Credentials

Pre-signed credential issuance follows a different validation contract from unsigned issuance. The signed payload is trusted as-is — UniCore only checks that the template exists, is `Published`, and that the format is compatible with the credential configuration.

| Rule                                       | Unsigned issuance | Pre-signed issuance |
| ------------------------------------------ | ----------------- | ------------------- |
| Template must be `Published`               | Yes               | Yes                 |
| Format must match credential configuration | Yes               | Yes                 |
| Payload validated against template schema  | Yes               | **No**              |
| `type` verified against template           | Yes               | **No**              |
| `credentialExpiration` enforced            | Yes               | **No**              |
| `expiresAt` in request honoured            | Yes               | **No (ignored)**    |

Content correctness is the caller's responsibility. If the template is no longer `Published` at redemption time, offer redemption fails. Holder-facing metadata uses the current live credential configuration; the signed content may diverge from the live template state, and this divergence is intentional and accepted.

---

## Validation Error Codes

Template API operations return two distinct HTTP error codes for input problems:

| Code                       | When to expect it                                                                                                                                                                                                                |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `400 Bad Request`          | Malformed JSON, deserialization failure, or other transport-level structural error                                                                                                                                               |
| `422 Unprocessable Entity` | Business rule violation — the request is structurally valid JSON but violates a domain constraint (e.g. invalid schema, forbidden status transition, missing required fields for publication, non-clearable field set to `null`) |

Business-rule failures fail fast rather than aggregating multiple errors into a single response.

---

## Practical Summary for New Integrators

1. **Create a template** with `status: Draft` and define the `schema` for your credential type.
2. **Test your schema** — UniCore validates schemas on write, so submission errors are caught early.
3. **Set `schemaPropertiesAttributes`** if you need selective disclosure (SD-JWT data models only).
4. **Publish the template** (`status: Published`) when it is ready for issuance.
5. **Issue credentials** by referencing the `templateId` in issuance requests. UniCore validates the submitted data against the template schema before issuing.
6. **Archive the template** when you want to stop new issuance without deleting history.
7. **Delete the template** (after archiving) if you want to permanently retire it.

---

## Related Documentation

- [Credential Issuance Features](../introduction/features/issuance.md)
- [ADR 0001: Keep `schemaPropertiesAttributes` as a Separate Field](../adr/0001-schema-properties-attributes-as-separate-field.md)
- [Configuration Reference](../configuration/CONFIGURATION.md)

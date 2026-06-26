# ADR 0001: Keep `schemaPropertiesAttributes` as a Separate Field

**Status**: Accepted  
**Date**: 2026-05-21  
**Context**: `feat/enforce-templates` branch

---

## Context

Templates contain a `schema` field that holds a standard JSON Schema document. This schema defines the input surface for credential issuance — what fields a caller must or may provide — and doubles as a client-side validation schema that external parties can use directly with off-the-shelf JSON Schema tooling.

Templates also carry claim attribute metadata, currently stored in a separate `schemaPropertiesAttributes` field. Each entry associates a leaf field path with two boolean flags:

- `selectivelyDisclosable` — whether the field may be selectively omitted by the holder in SD-JWT presentations
- `nonRemovable` — whether the field is mandated by a credential standard and must remain present in the schema (system-controlled, not settable by callers)

The question was: should this attribute metadata be embedded **inside** the JSON Schema using `x-` extension keywords (e.g. `"x-sd": true`, `"x-non-removable": true`), or kept as a **separate field alongside** the schema?

---

## Decision

Keep `schemaPropertiesAttributes` as a **separate top-level field**, independent of the `schema` field.

Address attribute entries by **JSON Pointer paths to leaf claims**, rather than by embedding metadata in object nodes or inventing a separate path syntax.

---

## Rationale

### 1. External parties validate the schema directly

The primary purpose of the `schema` field is to give API consumers and frontends a standards-compliant JSON Schema they can use to validate credential submissions before sending them to the API. This is an explicit product requirement.

External parties are expected to parse and validate the schema using standard JSON Schema tooling (e.g. `ajv`, Python's `jsonschema`, browser-side validators). These tools follow the JSON Schema specification strictly. While the specification allows `x-` keywords to appear without breaking validation, many real-world tools emit warnings on unknown keywords, and some strict-mode validators may reject them. Embedding platform-specific metadata inside the schema would compromise this primary purpose.

### 2. Separation of concerns

The `schema` field describes **what data looks like** — it is a structural contract between the issuer and credential submitters. The `schemaPropertiesAttributes` field describes **how the platform should handle that data** — it is an operational policy used internally for SD-JWT disclosure and standard-conformance enforcement.

Mixing these two concerns inside a single JSON document creates semantic confusion. A third-party developer reading the schema should not need to parse or understand platform-specific metadata to use the schema for its primary purpose.

### 3. Deterministic claim addressing

Claim metadata is meaningful at the level of actual claim values, not merely at the level of object containers. A nested object such as `address` or `achievement.criteria` is structure; the meaningful claim surface is the set of leaf values beneath it.

Using JSON Pointer paths to leaf claims gives UniCore a standard, deterministic way to refer to those claims:

- `/name`
- `/address/city`
- `/achievement/criteria/narrative`

This choice avoids ambiguity in nested schemas, aligns with how frontends derive fields from the schema tree, and removes the need for a UniCore-specific path notation.

### 4. Clean extensibility

Adding new attribute flags in the future (e.g. a `mandatory` flag for required submission, or a `pattern` override hint) is straightforward when attributes live in their own field. Embedding these in the JSON Schema would require every consumer to understand and ignore an ever-growing set of `x-` keywords.

### 5. Simpler schema storage and versioning

JSON Schema documents are often stored, cached, or forwarded verbatim by tooling. A schema that contains only standard JSON Schema keywords is safe to store and forward without stripping. A schema with embedded `x-` metadata requires either stripping before forwarding or ensuring downstream consumers handle unknown keywords gracefully.

---

## Alternatives Considered

### Embed as `x-` extension keywords

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "x-sd": true,
      "x-non-removable": false
    }
  }
}
```

**Rejected because**: third-party JSON Schema tooling may warn or fail on unknown keywords, compromising the primary use case of the schema field as a client-side validation document.

### Use JSON Schema `$comment` or `description` fields for metadata

**Rejected because**: `$comment` and `description` are unstructured strings; parsing structured metadata out of them is fragile and non-standard.

---

## Consequences

- The `schema` field remains a clean, standards-compliant JSON Schema document with no platform-specific extensions.
- `schemaPropertiesAttributes` keys use **JSON Pointer** notation (RFC 6901) to address leaf fields within the nested schema, providing an unambiguous and standard path format.
- Object container nodes are not directly addressable through `schemaPropertiesAttributes`; metadata is attached to leaf claims only.

---

## Note: No automatic `additionalProperties` injection

An earlier implementation automatically injected `"additionalProperties": false` into every object node of the stored schema. This was removed in favour of simplicity and readability. The schema is now stored exactly as provided by the caller. Callers who want strict additional-property rejection may include `"additionalProperties": false` themselves.

- Consumers who want to understand claim projection metadata must read both fields. This is an acceptable tradeoff given the audience (issuer-facing admin API, not credential holder tooling).
- When displaying the schema in a frontend form builder, the UI layer is responsible for merging the two fields into a combined view if needed.

## Related Documentation

For the current shape and behavior of templates across supported data models, see [Template Model](../template-model.md).

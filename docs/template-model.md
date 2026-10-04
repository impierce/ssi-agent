# Template Model

This document describes how templates are modeled today in UniCore, why they are structured this way, and which parts are deliberate design choices versus current implementation limitations.

This is not a frozen public contract. It describes the current template model as implemented in the codebase so contributors, frontend developers, and API consumers can understand the representation and its trade-offs.

For the architectural decision behind keeping claim metadata separate from the JSON Schema, see [ADR 0001](./adr/0001-schema-properties-attributes-as-separate-field.md).

## Overview

A template is modeled as three coupled parts:

1. `schema`: the structural shape of credential input data
2. `schemaPropertiesAttributes`: metadata for individual claim fields
3. `dataModel` and `type`: model-specific rules that change what is accepted and how it is normalized

The important point is that these parts are not independent. A schema may be valid JSON Schema and still be rejected because its claim metadata does not match the schema structure, or because its selected data model imposes stricter rules.

## Why Templates Are Modeled This Way

Templates serve two audiences at the same time:

- API callers and frontends need a machine-readable description of what data they may or must submit.
- UniCore itself needs extra metadata about how individual claims behave during issuance and selective disclosure.

The template model therefore separates structural data shape from platform-specific claim behavior.

### Design choice: JSON Schema as the structural surface

The `schema` field uses standard JSON Schema because it gives callers a well-known format that can be validated with off-the-shelf tooling before the API is called.

This keeps the submission surface legible and interoperable. A caller can reason about the required input fields without first learning UniCore-specific semantics.

### Design choice: claim metadata is outside the schema

The `schemaPropertiesAttributes` field exists because selective-disclosure behavior and similar platform concerns are not part of standard JSON Schema semantics.

Keeping that metadata outside the schema avoids mixing two different concerns:

- what the credential input looks like
- how UniCore should treat particular claims

### Design choice: metadata targets leaf claims

Claim metadata is attached to leaf fields rather than object container nodes. That choice reflects how claim values are actually issued and disclosed.

Objects provide structure, but the meaningful claim surface is the set of leaf values such as `/name`, `/address/city`, or `/achievement/criteria/narrative`.

### Design choice: JSON Pointer as the path format

`schemaPropertiesAttributes` keys use RFC 6901 JSON Pointer. This gives a standard, unambiguous way to refer to nested claims and avoids inventing a UniCore-specific path syntax.

It also keeps frontend and backend mapping deterministic because nested claim paths can be derived directly from the schema tree.

## Core Structure

## `schema`

The `schema` field describes the structure of credential input data.

A typical schema looks like this:

```json
{
  "type": "object",
  "properties": {
    "name": { "type": "string" },
    "address": {
      "type": "object",
      "properties": {
        "city": { "type": "string" }
      }
    }
  }
}
```

### Current behavior

- Root object schemas are supported.
- Nested object properties are supported.
- Scalar leaf fields such as `string`, `integer`, `number`, and `boolean` are supported.
- Standard JSON Schema keywords such as `$schema`, `format`, and `const` are accepted when the JSON Schema validator accepts them.
- For some data models, `schema` may be omitted entirely.

### Current limitation

Arrays are not currently supported anywhere in template schemas. This is an implementation limitation, not a conceptual statement that credentials can never contain repeatable values.

The current model avoids arrays because claim metadata and disclosure addressing are defined in terms of stable leaf paths, and array item addressing has not yet been introduced into that model.

## `schemaPropertiesAttributes`

The `schemaPropertiesAttributes` field stores metadata for individual claim fields.

Example:

```json
{
  "/name": {
    "selectivelyDisclosable": true,
    "nonRemovable": false
  },
  "/address/city": {
    "selectivelyDisclosable": false,
    "nonRemovable": false
  }
}
```

Each entry contains:

- `selectivelyDisclosable`: whether the claim may later be omitted during selective disclosure
- `nonRemovable`: whether the claim is treated as system-protected and must remain present once marked immutable

### Why it is structured this way

This field is separate because the metadata is operational policy, not structural validation. It tells UniCore how to treat claims, not what JSON shape a caller must submit.

### Why keys point only to leaf fields

Metadata is attached only to leaf claims because those are the values that are actually issued, validated, and selectively disclosed. Container objects are organizational structure, not claims in their own right.

### Current behavior

- Keys must be RFC 6901 JSON Pointer paths.
- Keys must point to existing leaf fields in `schema.properties`.
- Keys that point to intermediate object nodes are rejected.
- Keys that do not match the schema are rejected.
- Keys are trimmed during update validation, and collisions after trimming are rejected.

### Current limitation

Because arrays are unsupported, there is no array-item addressing model in `schemaPropertiesAttributes` yet.

## Relationship Between `schema` and `schemaPropertiesAttributes`

The two fields are intentionally separate but must stay aligned.

- `schema` defines which leaf claims exist.
- `schemaPropertiesAttributes` may only describe those leaf claims.

In practice, this means schema changes can invalidate attribute keys, and some schema updates may prune attributes or reject removal of immutable claims.

## Data Model Variants

The template model is shared across all data models, but each data model applies different constraints.

## W3C VC 1.1

### Current structure

- `schema` may be present or omitted.
- `type` is normalized to include `VerifiableCredential`.

### Design choice

W3C VC 1.1 templates do not expose claim-level metadata through `schemaPropertiesAttributes`.

### Current behavior

- `schemaPropertiesAttributes` is not allowed.

## W3C VC 2.0

### Current structure

- `schema` may describe nested object-based claim structures.
- `schemaPropertiesAttributes` may describe leaf claims.

### Design choice

This model exposes both structure and claim behavior because it is intended to support richer selective-disclosure-aware credential modeling.

### Current behavior

- `type` is normalized so `VerifiableCredential` is present and ordered first.
- Duplicate type entries are removed.
- `schemaPropertiesAttributes` must match leaf schema paths.

## Open Badges 3.0

### Current structure

Open Badges 3.0 uses the shared template representation, but imposes stricter schema and type rules.

A minimal acceptable structure looks like this:

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

### Design choice

Open Badges templates are stricter because the model is anchored to a specific credential standard. UniCore uses that stricter model to keep template authors aligned with the required Open Badges shape and to guarantee the presence of standard-mandated claims.

### Current behavior

- `type` must resolve to either `["VerifiableCredential", "OpenBadgeCredential"]` or `["VerifiableCredential", "AchievementCredential"]`.
- Extra Open Badges types are rejected.
- Required achievement fields must be present.
- Disallowed Open Badges schema properties are rejected.
- Required paths are marked non-removable by the backend.
- The stored schema is normalized with injected `required` arrays for required paths.

### Current limitation

Some of the Open Badges handling is current implementation behavior rather than a generalized template mechanism. In particular, required-field injection and auto-population of required claim metadata are current normalization steps performed specifically for this model.

## European Learning Model 3.3

### Current structure

- `schema` may describe nested object-based claim structures.
- `schemaPropertiesAttributes` may describe leaf claims.

### Current behavior

- `type` is normalized to include `VerifiableCredential`.
- `schemaPropertiesAttributes` follows the same leaf-path model as W3C VC 2.0.

## Normalization Behavior

The backend may normalize parts of a submitted template before storing or returning it.

### Current behavior

- `type` entries are trimmed, deduplicated, and canonically ordered.
- Tags are trimmed and deduplicated.
- Open Badges schemas may gain required lists.
- Open Badges attributes may be auto-populated and required fields marked non-removable.
- Empty attribute maps may be normalized away.

### Practical consequence

Callers should treat the backend response as the canonical saved representation instead of assuming the submitted payload is preserved byte-for-byte.

## Current Limitations Summary

The most important current limitations are:

- arrays are unsupported in template schemas
- claim metadata is defined only for leaf paths
- some model-specific normalization, especially for Open Badges, is implemented as backend behavior rather than exposed as a more general template-policy layer

These limitations are documented here because they materially affect how templates behave today. They should not be read as promises about the final long-term shape of the model.

## Relationship to ADR 0001

ADR 0001 explains the architectural decision to keep `schemaPropertiesAttributes` separate from `schema`, and why leaf claims are addressed using JSON Pointer.

This document explains how that decision is applied in the current template model, including data-model-specific constraints, normalization behavior, and present-day limitations.

# ADR 0012: Lists and Whole-Object Attributes in Template Schemas

**Status**: Accepted  
**Date**: 2026-11-18  
**Context**: Template `schema` and `schemaPropertiesAttributes`. Amends
[ADR 0001](./0001-schema-properties-attributes-as-separate-field.md), which addressed attributes to
leaf claims only.

---

## Context

A template author groups fields: an object of fields, a list of values, a list of objects, a table
(a list of rows with the same columns). To the author such a group is one field. It has one set of
attributes and is disclosed as one.

Two rules stood in the way:

- A template schema with `"type": "array"` anywhere was rejected. Array support was deferred until
  there was an answer to how attributes and claim paths address what is inside an array.
- A `schemaPropertiesAttributes` key had to address a leaf. A key for an object, such as
  `/address`, was rejected.

## Decision

- **Arrays**: the properties of a template schema may be arrays at any depth, of scalars or of
  objects, for every data model except Open Badges 3.0. The schema itself stays an object. UniCore stores the schema as given, with the keywords it
  attaches no meaning to (`examples`, an unknown `format`, ...). JSON Schema validation of the
  claims enforces `items`, `minItems`, `maxItems` and `const`.
- **Attribute keys address properties**: a key is a JSON Pointer that walks `properties` to a leaf,
  an object or an array. An attribute on an object or an array is for the whole of it.
- **Nothing inside an array is addressable**: a key never leads into `items`, nor through
  `properties` an array schema may carry. There is no index and no wildcard.
- **Issuer metadata**: the leaves of nested objects are claims, an array is one claim at its own
  path. This is what `credential_configuration_projection` already did.
- **Issuance**: unchanged. An SD-JWT credential conceals its claims one level deep: each property
  of `credentialSubject` in `vc+sd-jwt`, each top-level claim of its own in `dc+sd-jwt` (registered
  claims such as `vct` stay in the clear). An object, a list or a table at that level is one
  disclosure, with everything inside it.

## Rationale

- **A group is disclosed as one today.** Concealing stops one level into the claims. An attribute
  for the group matches that unit; attributes on its leaves describe a granularity UniCore does not
  have.
- **Objects and lists of objects behave alike.** An array was already a leaf to the key check, so
  `/positions` (a list of objects) would have been addressable while `/address` (one object) was
  not.
- **JSON Pointer stays the only path syntax.** ADR 0001 chose it to avoid a UniCore-specific
  notation. Items repeat, so an attribute per index means nothing, and "every item" needs a
  wildcard that JSON Pointer does not have.
- **Open Badges keeps its rules.** The standard gives the shape of the credential, and UniCore
  checks an Open Badges schema property by property against it. Arrays there need their own
  mapping.
- **No migration.** Keys to leaves stay valid, also below an object, and Open Badges templates
  still get their attributes per leaf. Stored templates and events are untouched.

## Alternatives Considered

### Keep leaf-only keys, one attribute per leaf of an object

**Rejected because**: several attributes would have to agree to say one thing, and a list of
objects would still be addressed as a whole.

### Address array items with indices or wildcards

**Rejected because**: it needs a path syntax beyond JSON Pointer, for a granularity of disclosure
that does not exist yet.

## Consequences

- A template can describe credentials with nested objects, lists and tables, and credentials are
  issued from it in every format a template gives: `jwt_vc_json` and `vc+sd-jwt`.
- An object and a property inside it can both carry an attribute. UniCore stores both. Which one
  governs disclosure is left open until UniCore discloses deeper than one level.
- A schema update that removes an object or an array drops its attribute. A non-removable one
  cannot be removed, like a non-removable leaf.
- A public offer needs every leaf fixed by `const`. A list or a table with a `const` is fixed; one
  without is not, whatever its items say.
- Disclosing single items of a list, or single fields of a nested object, needs a new decision.

## Related Documentation

[Template Model](../template-model.md) describes the current shape and behavior of templates.

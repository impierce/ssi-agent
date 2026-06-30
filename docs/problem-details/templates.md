## Missing Title

A title is required for the template.

## Invalid Schema Properties Attributes

The provided attribute keys (`schema_properties_attributes`) do not match the properties defined in the JSON Schema (`schema.properties`).

## Invalid Required Property Type

The type of the required property must be `string` or `const`.

## Missing Required Open Badges Properties

The provided properties do not include all required properties as defined in the Open Badges specification.
Currently, the following properties are required:

- **achievement.name**
- **achievement.description**
- **achievement.criteria.narrative**

## Disallowed Open Badges Properties

At least one of the provided properties is not part of the Open Badges specification.

## Non-removable Property Violation

This property can not be removed from the template.

## Invalid Status Transition

The template's status cannot transition to the requested state. Refer to the template lifecycle for valid transitions.

## Archived Template Immutable

An archived template is immutable and cannot be modified, except to change its status.

## Deleted Template Terminal

A deleted template is in a terminal state and cannot be modified in any way.

## Archive Before Delete Required

A published template must first be archived before it can be deleted.

## Invalid Expiration

The provided `credentialExpiration` value is not a valid ISO 8601 duration (e.g. `P90D`) or datetime (e.g. `2026-12-31T23:59:59Z`).

## Invalid Type

The provided `type` value is not a valid credential type string.

## Invalid Status On Create

Only `draft` or `published` are valid statuses when creating a template.

## Schema Properties Attributes Not Allowed

`schemaPropertiesAttributes` is not supported for W3C VC 1.1 templates.

## Duplicate Schema Properties Attribute Key

Two or more entries in `schemaPropertiesAttributes` resolve to the same path after normalisation (trimming leading/trailing slashes).

## Draft Template Cannot Be Public

A template with `visibility: public` must not be in `draft` status.

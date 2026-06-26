## Source Template Not Found

This error arises when the source template data for duplication can not be fetched. This could indicate that the `sourceTemplateId` used for duplication is not associated with an existing template. It can also arise in instances where the template has been deleted.

## Template ID Missing

This error arises when the template `id` field is left empty in the request body. The `id` is required to update or delete a template.

## Template Not Found

This error arises when the given `id` does not match any existing template in the library.

## Invalid Status Transition

This error occurs when a template is moved to a status that is not allowed from its current status (such as moving an `Archived` template back to `Draft`).

## Invalid Schema Properties Attributes

This error occurs when one or more keys in `schemaPropertiesAttributes` are invalid for the current schema.

## Non-removable Property Violation

This error occurs when an update tries to remove schema properties that are marked as non-removable/immutable.

## Disallowed Open-Badges Properties

This error occurs when an OpenBadges 3.0 template schema contains properties that are not part of the specification.

## Missing Required Open-Badges Properties

This error occurs when an OpenBadges 3.0 template schema is missing mandatory properties as defined in the specification.

## Invalid Required Property Type

This error occurs when required OpenBadges 3.0 properties are present but use an invalid type.

## Missing Title

This error occurs when a template create or update request omits the required `title`.

## Archived Template Immutable

This error occurs when attempting to mutate an archived template in a way that is not permitted.

## Deleted Template Terminal

This error occurs when attempting to mutate a template that is already in the deleted terminal state.

## Archive Before Delete Required

This error occurs when trying to delete a published template before archiving it first.

## Invalid Expiration

This error occurs when a template expiration value is malformed or unsupported.

## Invalid Type

This error occurs when the template `type` content is invalid for the selected data model or constraints.

## Invalid Status On Create

This error occurs when a new template is created with a disallowed initial status.

## Schema Properties Attributes Not Allowed

This error occurs when `schemaPropertiesAttributes` are provided for template variants where they are not supported.

## Duplicate Schema Properties Attribute Key

This error occurs when `schemaPropertiesAttributes` contains duplicate keys after normalization/trimming.

## Draft Template Cannot Be Public

This error occurs when a template in `Draft` stage is made public.

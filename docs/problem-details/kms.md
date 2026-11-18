## Key Provider Already Added

This error occurs when `add-key-provider` is called for a provider type that is already added. An instance connects to at most one provider of each type.

## Key Provider Not Found

This error occurs when a request names a provider type that hasn't been added, or has been removed.

## Key Provider Development Only

This error occurs when the in-memory provider is added outside the `development` profile. Its keys are lost on restart, so it is only for development and tests.

## Missing Key Provider Name

This error occurs when `add-key-provider` is called with a name that is empty once trimmed.

## Key Provider Name Taken

This error occurs when another added provider already has this name. Names are unique among the providers an instance lists.

## Key Provider Has Keys

This error occurs when `remove-key-provider` is called for a provider that still has keys. Removing it would leave its keys without the provider that holds them. Delete the keys with `delete-key` first, then remove the provider.

## Key Provider Not Supported

This error occurs when this instance has no adapter for the provider type.

## Key Provider Unreachable

This error occurs when the provider can't be reached with the credentials of this instance, such as when its sign-in is refused.

## Key Provider Error

This error occurs when the provider fails a request that should have worked. The message is the provider's.

## Key Already Exists

This error occurs when `create-key` is called with a label that already names a key in the provider. A retried request whose first attempt created the key gets this error instead of a second key. When this instance never recorded that key, such as when its creation was interrupted, record it with `adopt-key`.

## Key Already Recorded

This error occurs when `adopt-key` names a key that this instance has already recorded.

## Key Not Found

This error occurs when `delete-key` names a key that doesn't exist or was deleted, or when `adopt-key` names a label no key in the provider has.

## Key Not Supported By Provider

This error occurs when the provider can't create a key of the requested type or algorithm.

## Key Algorithm Refused By Provider

This error occurs when the provider can sign with the requested algorithm, but not in a way credentials can rely on. AWS KMS signs at most 4096 bytes with an Ed25519 (`EdDSA`) key, as it must be given the whole message, which credentials such as Open Badges with embedded images exceed. Create an `ES256` key instead.

## Unsupported Key Algorithm

This error occurs when keys can't be created for the requested algorithm at all.

## Unsupported Key

This error occurs when `adopt-key` names a key that is neither a P-256 nor an Ed25519 key.

## Invalid Key Label

This error occurs when the provider can't use the label to name a key, such as a label with characters its names don't allow.

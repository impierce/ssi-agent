## Connection Not Found

This error occurs when a request references a connection_id which does not match existing connections in the system. The specified `connection_id` may have been deleted, never created, or is incorrect.

## Credential Issuer Metadata Fetch Failed

This error occurs when the system is unable to retrieve credential issuer metadata from the connection's `/.well-known/openid-credential-issuer` endpoint. This may be caused by the remote server being unreachable, the URL being incorrect, or the endpoint not returning a valid metadata response.

## Missing Domain

This error should not occur as the domain is required to create a connection.
This error indicates that a connection is missing a required `domain` value. The domain is necessary for establishing and verifying the connection, and without it the operation cannot proceed.

## Opaque Origin Not Supported

This error occurs when an origin supplied to `add-linked-domains` or `remove-linked-domains` has no host at all, such as a `data:` URL. A linked domain must identify a public host.

## Host Must Be A Domain Name

This error occurs when an origin supplied to `add-linked-domains` or `remove-linked-domains` is an IP address. Domain linkage requires a domain name, not an IP address.

## Invalid Domain Or Origin

This error occurs when an origin supplied to `add-linked-domains` or `remove-linked-domains` could not be parsed as a bare host (e.g. `example.org`) or a full origin (e.g. `https://example.org`). A single invalid entry rejects the whole request.

## No Origins Given

This error occurs when `add-linked-domains` or `remove-linked-domains` is called with an empty `origins` array. At least one origin is required; there is no request that links or unlinks nothing.

## Signing Key Failed

This error occurs when a key of your organisation's DID document can't sign, such as when its key provider can't be reached or the key was disabled in or deleted from it. The message names the verification method and the key. UniCore doesn't check keys in their provider before they sign, so until a signature fails like this one, `get-signing-keys` shows a deleted or disabled key as available, without a `problem`. After the failure, `get-signing-keys` shows the key's `problem`, the configured key of its algorithm signs instead of it, and UniCore retries the key in the background until it signs again. To stop using the key, select another key with `select-signing-key`, or the configured one with `deselect-signing-key`. Domain linkage signs with every key of the document, so a key that can't sign stops it until the key is removed with `remove-key-from-document`.

## Not An Added Key

This error occurs when `remove-key-from-document` names a verification method that wasn't added with `add-key-to-document`, such as one of the configured keys.

## Selected Signing Key

This error occurs when `remove-key-from-document` names the key selected to sign for its algorithm. Removing it would leave the algorithm without the key it signs with. Select another key with `select-signing-key`, or the configured one with `deselect-signing-key`, then remove it.

## Key Unusable

This error occurs when `add-key-to-document` names a key that exists, but that this instance can't sign with, such as one whose key provider it has no adapter for, or whose provider can't sign with the algorithm the key was created for. The message names the key and the reason. Trying again gives the same answer: add a key this instance can sign with instead.

## Key Unavailable

This error occurs when `add-key-to-document` names a key that can't be had right now, such as when its key provider doesn't answer or the key's record can't be read. The message names the key and the reason. Nothing was added; try again later.

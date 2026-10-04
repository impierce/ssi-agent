# Holder

These problems can occur while receiving credentials or creating presentations in the holder API.

## Credential Decoding Failed

The supplied credential could not be decoded as a JWT verifiable credential. Ensure that the credential is a well-formed JWT and that it is supplied in the format expected by the holder endpoint.

## Invalid Credential Status

The credential's status could not be verified, or the credential is no longer valid. Check the credential's status information and retry with a valid, non-revoked credential.

## Credential Offer Not Found

No received credential offer exists for the supplied ID. Retrieve the offer first, then use the ID returned by that operation.

## Credential Offer Not Pending

The credential offer has already been accepted or rejected. An offer can only be accepted or rejected while it is pending.

## Credential Offer Not Accepted

The credential offer must be accepted before its credentials can be requested. Accept the offer, then retry the credential request.

## Missing Token Response

The accepted credential offer has no token response, so its credentials cannot be requested. Obtain an access token for the offer before requesting credentials.

## Missing Pre-Authorized Code

The credential offer does not contain a `pre-authorized_code` grant. UniCore currently supports only that grant type for this holder flow.

## Missing Credential Configurations

The credential offer does not advertise any credential configurations. Check the issuer's offer and metadata.

## Missing Credential Configuration

A credential configuration referenced by the offer is absent from the issuer metadata. Check that the issuer publishes metadata consistent with the offer.

## Credential Offer Retrieval Failed

UniCore could not retrieve a credential offer referenced by `credential_offer_uri`. Check that the URI is reachable and returns a valid credential offer.

## Credential Issuer Metadata Retrieval Failed

UniCore could not retrieve the credential issuer metadata. Check the issuer URL, its `/.well-known/openid-credential-issuer` endpoint, and the issuer's availability.

## Authorization Server Metadata Retrieval Failed

UniCore could not retrieve the authorization server metadata required by the credential offer. Check the issuer metadata and the authorization server's discovery endpoint.

## Missing Token Endpoint

The authorization server metadata does not advertise a `token_endpoint`. The issuer or authorization server must publish one for this flow.

## Token Request Failed

The upstream authorization server rejected the token request or could not be reached. Inspect its response and retry after correcting the offer or authorization-server configuration.

## Credential Request Failed

The upstream credential issuer rejected the credential request or could not be reached. Inspect its response and retry after correcting the request or issuer configuration.

## Unsupported Deferred Credential Response

The issuer returned a deferred credential response. UniCore does not yet support deferred credential retrieval.

## Unsupported Batch Credential Request

The requested flow requires a batch credential request. UniCore does not yet support requesting multiple credentials in one request.

## Unsupported Credential Format

The issuer offers a credential format other than JWT. UniCore currently supports JWT credentials in this holder flow.

## Missing Holder Identifier

The holder has no identifier available when building a presentation. This is an internal holder configuration problem; configure or create the holder identity before retrying.

## Invalid Holder Identifier URL

The holder identifier cannot be represented as a valid URL while building a presentation. This is an internal holder configuration problem; correct the holder identity before retrying.

## Presentation Build Failed

UniCore could not construct the presentation from the holder's credential and presentation data. This is an internal holder configuration or data problem; inspect the accompanying error detail.

## Presentation Serialization Failed

UniCore could not serialize the presentation. This is an internal holder configuration or data problem; inspect the accompanying error detail.

## Missing Signing Key Identifier

UniCore could not find the signing key identifier required to sign the presentation. Configure the holder's signing key before retrying.

## Presentation Signing Failed

UniCore could not sign the presentation with the holder's key. Verify the holder's key configuration and inspect the accompanying error detail.

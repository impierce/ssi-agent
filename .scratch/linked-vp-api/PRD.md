# Linked VPs can't be added or removed one at a time

Status: ready-for-agent

## Problem

Holders publish credentials on their profile as linked verifiable presentations (linked VPs). Each linked VP gets an entry in the `linked-verifiable-presentation-service` service of the DID document. unitrust-ui publishes one linked VP per credential, so it needs to add or remove a single presentation ID without changing the others.

unitrust-ui still calls `POST /v0/services/linked-vp`, which replaced the whole list. #394 replaced the path-style routes with command-style ones, and that route is gone. Two linked-VP commands exist now:

| Endpoint                                         | Behavior                                    |
| ------------------------------------------------ | ------------------------------------------- |
| `POST /v0/create-linked-verifiable-presentation` | Creates the service. `409` if it exists.    |
| `POST /v0/remove-linked-verifiable-presentation` | Deletes the whole service. `404` if absent. |

That leaves no way to add a second presentation. The only workaround is to delete the service and create it again with the new list. That has two problems:

- **It isn't atomic.** If the create fails, every linked VP drops off the profile.
- **It races.** Two concurrent publishes both read the list, and the last write wins, so the other publish is lost.

What production shows: the create call succeeds, and the publish call gets `405` with an empty body. The UI logs:

```
Failed to update the published linked VPs: {}
Linked VP `<id>` was created but could not be published
```

## Compatibility

None required. ssi-agent may break every consumer, unitrust-ui included: no deprecation period, no compatibility shims, no aliases for old routes, no event upcasting. Choose the cleanest API and data model. Consumers adapt afterwards.

Assume a clean database. No stored events or other data have to be kept, so events that no longer deserialize are not a concern.

## Proposed fix

Mirror the linked-domains API, which already has idempotent, incremental semantics (`AddLinkedDomains` / `RemoveLinkedDomains` in `agent_identity/src/service/command.rs`).

### Commands

Replace `CreateLinkedVerifiablePresentationService` / `DeleteLinkedVerifiablePresentationService` with:

```rust
/// Publishes the given presentations, in addition to any already published. Adding one that is
/// already published is a no-op. Creates the service if it does not exist yet.
AddLinkedVerifiablePresentations {
    service_id: String,
    presentation_ids: Vec<String>,
},
/// Stops publishing the given presentations. Removing one that is not published is a no-op.
/// Removing the last remaining one deletes the service.
RemoveLinkedVerifiablePresentations {
    service_id: String,
    presentation_ids: Vec<String>,
},
```

Operations: `identity.services.linked_verifiable_presentation.add` / `.remove`.

The aggregate computes the new list from its current state. Handling the command is then the only read-modify-write, so the race goes away. Emit one event that carries the full resulting list and `is_deleted`, like `LinkedDomainsRemoved` carries the full resulting origins. Because each DID document gets its own entry (see [Publishing per DID document](#publishing-per-did-document)), the event carries the presentations rather than one shared `service` entry:

```rust
pub struct LinkedPresentation {
    pub presentation_id: String,
    /// The DID that signed the presentation and is the subject of its credentials.
    pub holder: String,
    /// The absolute URL the presentation is served at, fixed when it was added.
    pub url: Url,
}
```

Replace `Service::presentation_ids: Vec<String>` with `presentations: Vec<LinkedPresentation>`. The URL is stored rather than derived later, so that the event alone describes what was published, even if `public_url` changes afterwards.

#### Order and duplicates

- The list keeps insertion order: the IDs already published come first, then the new ones in request order. Don't sort.
- Every ID appears in the list at most once. An ID that is already published is skipped. An ID repeated within one request is added once, at its first position.
- Removing IDs keeps the remaining ones in their existing order.
- If the resulting list equals the current one, emit no event (like `AddLinkedDomains`).

#### Presentation check

`AddLinkedVerifiablePresentations` only publishes a presentation that a verifier would accept. The agent already verifies other holders' linked VPs (`validated_credentials` in `agent_identity/src/services.rs`), so reuse the same `JwtPresentationValidator` path. Don't write a second validator. For each ID:

1. **It exists and is signed.** Otherwise the published URL `/linked-verifiable-presentations/{id}` returns `404` (`agent_api_http/src/v0/holder/holder/presentations/presentation_signed.rs`). Error: `PresentationNotFound(id)`.
2. **Its holder DID has a document that can publish it.** Read the holder DID from the presentation. That DID's document must be one this agent controls, is enabled, and passes `can_link`. Otherwise the presentation would be published nowhere. Error: `PresentationInvalid(id, reason)`.
3. **Its signature verifies against that document.** Error: `PresentationInvalid(id, reason)`.
4. **The holder DID is the subject of every embedded credential.** The DIF spec asks verifiers to check this, so a presentation that fails it would be rejected once published. Error: `PresentationInvalid(id, reason)`.

If any ID fails, the whole command fails and nothing is published.

`agent_identity` depends on `agent_holder` only as a dev-dependency. Get the signed presentation JWT through a port on `IdentityServices`, for example a lookup from a presentation ID to its signed JWT. Implement the port where the application is wired together, using the holder's presentation query. Don't add a runtime dependency from `agent_identity` on `agent_holder`.

`RemoveLinkedVerifiablePresentations` skips the checks. Removing an ID that is not published is a no-op either way, and a user must always be able to withdraw a presentation.

#### Conformance with DIF Linked Verifiable Presentations

The format comes from the [DIF Linked Verifiable Presentation spec](https://identity.foundation/linked-vp/), not from an OpenID spec. The service entry has to follow it:

- `type` is exactly `LinkedVerifiablePresentation`.
- `id` is a valid URI. The DID-specific `#linked-verifiable-presentation-service` fragment set in `synchronize_services` meets this.
- `serviceEndpoint` is a URL string or a **non-empty** array of URLs. Never publish an empty array. That's why removing the last ID deletes the service. Keep always writing an array, even for one ID. The spec allows it, and the shape stays the same whatever the count.
- Each endpoint URL is absolute and resolvable: `{public_url}linked-verifiable-presentations/{id}`. Build it with `Url` path-segment APIs, which percent-encode the ID, instead of pasting it in with `format!`. This resolves the existing TODO. Presentation IDs are server-generated UUIDs, and check 1 rejects any other ID, so encoding is a safety net rather than a feature.
- The resource keeps being served as a JWT with `Content-Type: application/jwt`.

#### Publishing per DID document

`synchronize_services` adds every active service to every updatable, enabled DID document. A presentation is signed by one DID only, the preferred DID method at creation time (`agent_holder/src/presentation/aggregate.rs`). In a document with a different DID, the presentation fails verification, including by this agent's own verifier, which validates against the document that published the service.

So a linked VP is published only in the document of its holder DID:

- For each document, `synchronize_services` builds the `LinkedVerifiablePresentation` entry from the presentations whose `holder` is that document's DID, in list order. The fragment is `#linked-verifiable-presentation-service`, as today.
- A document that holds none of the presentations gets no entry. If it has one, remove it. This follows the non-empty `serviceEndpoint` rule.
- Presentations from different holders can be published at the same time. For example, after the preferred DID method changes, older presentations stay in the old DID's document and newer ones go into the new DID's document.
- If a holder's document is later disabled or stops passing `can_link`, its entry is left out of that document, like other services today. The presentations stay in the aggregate and reappear once the document can be updated again.
- Linked domains keep today's behavior: they're published in every document. Only the linked-VP service is filtered by holder.

`synchronize_services` must check whether the entry for each document is up to date, as it does for linked domains today. Otherwise it would re-publish unchanged documents.

#### Removed and renamed

Remove `CreateLinkedVerifiablePresentationService` / `DeleteLinkedVerifiablePresentationService` and their events (`LinkedVerifiablePresentationServiceCreated` / `...Deleted`) outright (see [Compatibility](#compatibility)).

The new events are `LinkedVerifiablePresentationsAdded` / `LinkedVerifiablePresentationsRemoved`, parallel to `LinkedDomainsAdded` / `LinkedDomainsRemoved`. Use these names everywhere the old ones appear:

- `ServiceEvent` in `agent_identity/src/service/event.rs`
- `ServiceEvent` in `agent_shared/src/config/mod.rs` (the event types that can be published)
- The `service` event list in `agent_event_publisher_http/README.md`
- Tests, fixtures and config examples that name the old events (`grep -rn LinkedVerifiablePresentationService`)

### HTTP

| Endpoint                                          | Body                              | Responses                  |
| ------------------------------------------------- | --------------------------------- | -------------------------- |
| `POST /v0/add-linked-verifiable-presentations`    | `{ "presentationIds": string[] }` | `204`, `401`, `403`, `422` |
| `POST /v0/remove-linked-verifiable-presentations` | `{ "presentationIds": string[] }` | `204`, `401`, `403`, `422` |

- Both are idempotent: no `409` and no `404`.
- An empty `presentationIds` is `422`. Declare `min_items = 1` in the schema, so that the generated `openapi.yaml` shows the rule too.
- Adding an ID that is unknown or not signed is `422`, with the problem type `presentation-not-found`. Adding one that fails verification or the subject check is `422`, with the problem type `presentation-invalid`. Both name the ID, and `presentation-invalid` also gives the reason.
- Annotate both with `utoipa`, with `operation_id`s `add_linked_verifiable_presentations` / `remove_linked_verifiable_presentations`, so that unitrust-ui can use generated client functions instead of a raw `client.post`. Document the order, duplicate and no-op rules in the doc comments, as `add_linked_domains` does.

Delete the `create-`/`remove-linked-verifiable-presentation` routes and `LinkedVPEndpointRequest`.

### Leftovers of `/v0/services/linked-vp` to clean up

- `agent_api_http/bruno/gen/Identity/Create a linked verifiable presentation service.bru` still targets `POST /v0/services/linked-vp` and describes the old `200` response. It is stale, so regenerate the Bruno collection from `openapi.yaml` (`cargo test generate_openapi_spec`, then `agent_api_http/bruno/generate.sh`).
- `agent_api_http/src/v0/identity/services/mod.rs:424` asserts that `services/linked-vp` returns `405`. It only guards a route nobody calls any more, so drop that assertion.
- `LinkedVPEndpointRequest` (Rust type and `openapi.yaml` schema) keeps the old "endpoint" naming. It goes away with the old routes.

### Tests

- Add to an empty service → the service is created and lists the ID.
- Add a second ID → both are listed, and the first is untouched.
- Add an existing ID → no-op, `204`.
- Add `[b, a]` after `[c]` → the list is `[c, b, a]`.
- Add `[a, b, a]` → the list is `[a, b]`.
- Add an unknown ID, or one whose presentation isn't signed → `422`. Nothing is published, even for the valid IDs in the same request.
- Add a presentation whose credential subject is another DID → `422 presentation-invalid`.
- Unit-test the endpoint builder with an ID containing `/`, `?`, `#` and a space. The URL must stay under `linked-verifiable-presentations/` as a single path segment.
- After an add, resolve the published DID document and validate every linked VP the way an external verifier would. Reuse `fetch_linked_vp_validations`, as the linked-domains test validates the published linkage. Every presentation and every credential must come out valid.
- The published service entry matches the DIF shape: `type` is `LinkedVerifiablePresentation`, and `serviceEndpoint` is a non-empty array of absolute URLs.
- With two enabled DID documents (for example `did:web` and `did:iota`), a presentation signed by one DID is published only in that DID's document. The other document has no linked-VP entry.
- Presentations from two different holders → each document lists only its own, in insertion order. Removing the last presentation of one holder removes the entry from that document only.
- Add a presentation whose holder DID has no document that can publish it (disabled, or no document for that DID) → `422 presentation-invalid`.
- Disable a holder's document and enable it again → its entry is removed, then published again with the same presentations.
- Add or remove an empty list → `422`.
- Two concurrent adds with different IDs → both are listed (like the linked-domains concurrency test in `agent_api_http/src/v0/identity/services/mod.rs`).
- Remove one of two → the other stays.
- Remove the last one → the service is gone from the DID document.
- Remove an unknown ID → no-op, `204`.
- The state survives a restart (extend `runtime_services_preserve_identity_and_survive_restart`).
- Authorization records the new operation names.

## unitrust-ui follow-up

unitrust-ui will be broken until this is done. That's accepted. Once this ships and `openapi.yaml` is regenerated:

- `src/lib/server/linked-vp.ts`: `publishLinkedVp` / `unpublishLinkedVp` call the new generated functions with `[linkedVpId]`. `replacePublished` goes away, and so does the read-before-write in both functions. Resolve the TODO at the top of the file.
- Log the HTTP status on failure. The generated client reports an empty error body as `{}`, which hid the `405`.

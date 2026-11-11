# ADR 0010: Template Origin Digest over Canonical Public Content

**Status**: Accepted  
**Date**: 2026-11-04  
**Context**: The optional `origin` of a template (`source`, `sourceTemplateId`, `digest`), recording
where it was imported from.

---

## Context

A template can be imported from the public templates of another agent. Its origin keeps a digest of
the content at import time. Comparing that digest with the digest of the source's current content
tells whether the source changed; comparing it with the digest of the local template tells whether
the copy was edited locally. Both sides must therefore compute the digest the same way, across
implementations and across versions of this code, and stored digests stay in event streams for good.

## Decision

- **Input**: the public content of a template, the fields of the public template projection
  (`GET /public/templates`) without `modifiedAt`. One type, `PublicTemplateContent` in
  `agent_library`, defines it. The projection is built from it by exhaustive destructuring, so a
  field added to one but not the other fails to compile.
- **Canonical form**: RFC 8785 (JCS) over the JSON of that content, with top-level `null` members
  left out. An absent optional field and a `null` one give the same digest, and adding an optional
  field later keeps the digests of content that does not set it. `null` inside the schema is kept.
- **Encoding**: `sha256-` followed by the lowercase hexadecimal SHA-256 of the canonical bytes. The
  prefix names the algorithm.
- **Pinning**: test vectors in `agent_library/src/template/public_content.rs`, computed with an
  independent implementation, cover key order, whitespace, UTF-16 key sorting, Unicode, escaping and
  number formatting.
- **Responsibility**: callers compute the digest of the source's content and pass it in the origin;
  this code validates its format, stores it and never fetches or verifies a source.

## Considered Options

1. **Hash of the raw response bytes**: depends on whitespace, key order and serializer, so two equal
   templates would differ. Rejected.
2. **`PublicTemplateDto` flattening `PublicTemplateContent`** (`#[serde(flatten)]`): one definition,
   but the generated OpenAPI schema of `PublicTemplate` turns into an `allOf`, which changes the
   generated client types. Rejected in favour of exhaustive destructuring.
3. **Keeping top-level `null` members**: simpler, but every new optional field would change every
   digest and mark every imported template as changed and edited. Rejected.

## Consequences

- Changing the public content (adding a non-optional field, renaming one, changing a serialization)
  changes digests: imported templates then report both a changed source and a local edit until they
  are re-imported. The test vectors fail first and make such a change visible.
- The digest detects change only: no ordering, no history, no authenticity.

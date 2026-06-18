# Public Credential Offers

## What this feature does

Public Credential Offers let an issuer publish a reusable credential offer link that anyone can use, without creating a new one-off offer each time.

This enables "share once, issue many" use cases.

## Why this is useful

- It reduces manual work for teams issuing the same static credential repeatedly.
- It gives a stable entry point that can be embedded in portals, emails, landing pages, or QR codes.
- It allows operators to pause, resume, or retire an offer without changing the original credential template.

## How it works (high level)

1. An issuer creates a normal offer and links it to a template to create a public offer.
2. The public offer is stored with a lifecycle status (active or inactive).
3. Clients can discover public offers and see how many successful issuances happened.
4. Operators can take an offer offline, bring it back online, or delete it.

## Built-in safeguards

Public offers are intentionally restricted to static credentials:

- The referenced template must exist.
- Deleted templates are treated as unavailable.
- The template schema must use const values for all leaf fields.

If a template is missing, creation fails with not found.
If a template schema contains non-const leaf fields, creation fails with a business validation error.

## API capabilities

This feature currently provides route-level capabilities to:

- Create a public offer mapping
- List all public offers
- Take a public offer offline
- Take a public offer online
- Delete a public offer

## Operational intent

Use Public Credential Offers when you want controlled, repeatable issuance of predefined credentials at scale.

Do not use this flow for credentials that require dynamic or user-specific claim values per issuance.

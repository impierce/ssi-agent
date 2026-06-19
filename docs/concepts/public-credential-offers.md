# Public Credential Offers

## What this feature does

Public Credential Offers let an issuer create a reusable credential offer link that anyone can use, without creating a new offer for each issuance.

This enables "share once, issue many" use cases (public events, demos, testing environments).

## Why this is useful

- The resulting QR code can be embedded in portals, emails, landing pages without it having to be recreated for each issuance.

## How it works (high level)

1. An issuer creates a template which has no user-specific fields (such as a name, a grade or alike) and only constant values that are the same for each credential to be issued (such as the name of the event, the date of issuance, a logo).
2. The public offer is created and can be taken online and offline by the operator. A public URL is created which the operator can share or show on a screen.
3. Clients can scan the QR code and claim the credential in their identity wallets.
4. The operator can track the amount of successful claims in the overview.
5. Operators can take an offer offline, bring it back online, or delete it.

## Built-in safeguards

Public offers are intentionally restricted to static credentials:

- The referenced template must exist.
- Deleted templates are treated as unavailable.
- The template schema must use const values for all fields.

## Operational intent

Use Public Credential Offers when you want controlled, repeatable issuance of predefined credentials at scale.

Do not use this flow for credentials that require dynamic or user-specific claim values per issuance.

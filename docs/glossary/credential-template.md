# Credential Template

A **credential template** serves as a blueprint for a verifiable credential. It tells UniCore what the credential should look like, what information it must contain, and under what conditions it may be issued.

Think of a credential template as the definition an issuer sets up once and reuses every time they issue a particular type of credential — for example, a diploma, a membership card, or a professional certification.

A template answers questions like:

- **What fields does this credential contain?** (e.g. name, date of birth, course title)
- **How long is the credential valid?** (e.g. 90 days, 1 year, or indefinitely)
- **Which fields can the holder choose to hide** when sharing the credential? (selective disclosure)
- **What credential standard does it follow?** (e.g. Open Badges 3.0 for achievement credentials)

Before a template can be used for issuance, it must be **published**. Templates move through a simple lifecycle — `Draft` while being set up, `Published` when active, `Archived` when retired, and `Deleted` when no longer needed.

→ For full details see [Concepts: Credential Templates](../concepts/credential-templates.md)

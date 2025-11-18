# The Definition of 'Done'

## 1. The Impierce Definition of Done (The Principle)

> **The Impierce DoD:** "Done" means ready for production, not just for review.

"Done" means you've been considerate of the next person. Your work is not "done" until:

* **A Reviewer** can understand *what* you did and *why* you did it.
* **A Tester** can understand *how* to verify it.
* **A Future Developer** (or you, in 6 months) can understand your code and its purpose.
* **The Product** is measurably better for it.

---

## 2. The New Pull Request (PR) Template (The Checklist)

Thanks for your contribution and hard work! Before you submit, please make sure you have read and filled out this template. You won’t just be helping out a teammate, but in the long run, you will be helping yourself!

### What's the purpose of this PR?

*(This is your first required point. It forces the developer to think about the "why" before the "what".)*

Please write a few sentences explaining the feature and how it furthers our products. This can be technical or business-focused.

**Examples:**
* "This PR implements the email delivery service, which unblocks the core issuance flow."
* "This updates our badge rendering to be 100% compliant with Open Badges v3, which is critical for our SURF and education partners."
* "This refactors the database connection logic to be more resilient, which will fix the connection pool errors we saw last week."

**[Your explanation here]**

### What changes did you make?

A high-level summary of the technical changes.

* [Brief summary of change 1]
* [Brief summary of change 2]
* ...

**Links to any relevant issues:**
* Fixes #[Issue Number]
* Related to #[Issue Number]

### How can this be tested?

Provide clear, step-by-step instructions for the reviewer to verify your work.

1.  `docker-compose up ...`
2.  Go to `localhost:xxxx`
3.  Click "Issue Credential" and...
4.  You should see the new "Sent via Email" status.
5.  Check your email inbox for...

### Developer's Ready-to-Merge Checklist

You are the first line of defense! Please review your own work and check off the following boxes before requesting a review.

#### Code Quality
- [ ] **Self-Review Done:** I have read through my own code, removed all `console.log`s, and cleaned up commented-out lines.
- [ ] **Clarity:** I've added comments to any hard-to-understand or complex logic.
- [ ] **No "Magic":** I have avoided "magic strings" or numbers; any new constants are in the `config/constants` file.

#### Testing
- [ ] **New Tests Added:** I have written new unit/integration tests that prove my feature works or my bugfix is effective.
- [ ] **All Tests Pass:** All new and existing tests pass locally (`npm test` or similar).
- [ ] **Manual Test Done:** I have successfully run and tested this change in a Docker environment (as described in "How can this be tested?").

#### Impact & Documentation
- [ ] **Documentation Updated:** (This is your second required point.) I have made corresponding changes to the documentation (e.g., Confluence, READMEs, API docs), or I have confirmed that no changes are needed.
- [ ] **Purpose Explained:** I have filled out the "What's the purpose of this PR?" section at the top.

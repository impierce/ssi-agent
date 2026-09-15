# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual strings used in this repo's issue tracker.

Because this repo tracks skill-generated issues as [local markdown files](./issue-tracker.md), a "label" here is the value of the `Status:` line near the top of an issue file — not a GitHub label.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), write the corresponding string from the right-hand column into the issue's `Status:` line.

The repo's GitHub labels (`Bug`, `Enhancement`, `Blocked`, `Wontfix`, the release labels `Added`/`Patch`/`BREAKING CHANGE`, …) are a separate vocabulary for PRs and public issues. Don't mix the two.

Edit the right-hand column to match whatever vocabulary you actually use.

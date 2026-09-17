# Issue tracker: Local Markdown

Issues and specs produced by the agent skills live as markdown files in `.scratch/`.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`
- The spec/PRD is `.scratch/<feature-slug>/PRD.md`
- Implementation issues are `.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01`
- Triage state is recorded as a `Status:` line near the top of each issue file (see [triage-labels.md](./triage-labels.md) for the role strings)
- Comments and conversation history append to the bottom of the file under a `## Comments` heading

`.scratch/` is **not** gitignored — these files are committed and shared with the team.

## When a skill says "publish to the issue tracker"

Create a new file under `.scratch/<feature-slug>/` (creating the directory if needed).

## When a skill says "fetch the relevant ticket"

Read the file at the referenced path. The user will normally pass the path or the issue number directly.

## Relationship to GitHub Issues

`impierce/ssi-agent` also has GitHub Issues, which remain the human- and community-facing tracker.
Skills write to `.scratch/` and do **not** create GitHub issues unless explicitly asked.

# Issue tracker: Linear

Issues and PRDs for this repo live as Linear issues in the **ENG** team. There is no dedicated Linear project for `imgchk` yet — leave the `project` field unset when creating issues, or ask the user if a project has since been created.

## Tooling

Use the **Linear MCP** tools when available (preferred). If MCP tools are not exposed in the current session, fall back to the Linear GraphQL API using `LINEAR_API_KEY` from the environment. Do not invent a CLI that isn't installed — check first with `which linear` / `which lr` before assuming.

## Conventions

- **Team**: `ENG` (use the team key, not the name, when the API requires an identifier)
- **Project**: none yet for `imgchk`; leave unset
- **Repo-scoped labels** (separate from triage labels):
  - `bug` — defects in existing behavior
  - `feature` — new user-facing capability
  - `chore` — refactors, deps, build, CI
  - `docs` — README, CHANGELOG, code comments
  - `tui` — anything touching `src/ui.rs` or keybindings
  - `oci` — image fetch, layer parsing, registry concerns
- **Triage labels**: see `triage-labels.md`

When creating an issue, apply at minimum one repo-scoped label (axis of work) plus the appropriate triage label.

## When a skill says "publish to the issue tracker"

Create a Linear issue in team `ENG`.

## When a skill says "fetch the relevant ticket"

Fetch the Linear issue by its identifier (e.g. `ENG-123`) including comments.

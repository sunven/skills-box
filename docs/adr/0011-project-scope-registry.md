# Project scope is an explicit, app-registered scope

Status: accepted

v1 managed global skills only, and deliberately ran the CLI in an isolated empty cwd so project scope could never appear. In `skills@1.7.4` project scope is defined by the process cwd rather than a flag: an install without `-g` writes `<cwd>/.agents/skills`, `<cwd>/skills-lock.json`, and `<cwd>/<agent.skillsDir>` for non-universal agents. We therefore add project scope as a first-class scope: the app keeps a registry of project roots in app data (manual folder-picker registration, no disk scanning), every read and mutation carries a scope id (global or project), and each scope owns its canonical store, lock, agent attachments, and update-check state. Project mutations run the pinned CLI with the project root as cwd and without `-g`; global mutations keep the isolated cwd and `-g`. Skill identity becomes (scope, name): global ids stay as they are, project ids are project-prefixed. Registered roots are the only additional roots the content commands may read. Attach, detach and install linking reuse the shared attachment target gate (ADR 0005); extra agents' project skills directories extend its target-path rules rather than introducing a second eligibility path.

## Considered options

- **Auto-scan the disk for project roots**: rejected because discovery becomes unpredictable, permission-heavy, and produces ghost projects.
- **Open a folder ad hoc without a registry**: rejected because it loses revisit, per-scope update state, badges, and a stable project identity.
- **Merge project skills into the global inventory**: rejected because same-name skills are independent and the scopes have different locks and attachment sets.
- **Invoke the CLI with arbitrary cwd per operation**: rejected for the same reason; there is no project identity to persist state against.

## Consequences

- ADR 0001's isolated-cwd rule becomes scope-conditional: global mutations keep the isolated cwd; project mutations use the project root.
- Command construction now derives Scope from the same layout as local verification and bookkeeping, rather than copied plan fields (see [ADR 0017](0017-cli-scope-from-layout.md)).
- The registry needs a lifecycle: an unavailable root is kept and marked, with relocate (same project identity) and forget (never deletes files).
- New project failures extend the typed error taxonomy (ADR 0006); no ad-hoc code strings.
- The security allowlist extends to registered project roots; forgetting a project revokes its read access.
- Extra agents need an optional project skills directory, since the vendored catalog only supplies catalog agents' project dirs.
- The single global update-check state becomes a per-scope state; forgetting or relocating a project drops its state.
- The UI shows one active scope at a time; there is no merged cross-scope list.

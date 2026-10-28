# CLI mutations, native reads

Status: accepted

The GUI's inventory must expose states the CLI does not: descriptions and lock timestamps, physical agent attachments, copy-only and unmanaged skills, stale lock entries, and local modifications. `skills list -g --json` omits those fields and over-reports agents, while the CLI's lock writes are non-atomic and its `update` / `check` paths overwrite local state. We therefore read the lock v3 file and filesystem directly in Rust and use the pinned `skills@1.7.4` CLI only as the mutation executor for install, update (rebuilt as `add`), and uninstall; detach is a native unlink. `list -g --json` is at most a cross-check.

## Considered options

- **CLI list as the read index**: rejected because it lacks required fields, over-reports universal agents, and depends on Node for read-only use.
- **Native implementation of install / update / uninstall**: rejected for v1 because it would duplicate the CLI's source resolution, lock, and linking semantics; the CLI remains the safer mutation executor.
- **Hybrid reads with CLI fallback**: kept only as a diagnostic cross-check, not as a source of truth.

## Consequences

- The read-only inventory works without Node; mutations still require Node, npx, and git.
- The app must vendor the 79-agent catalog and detection rules from the pinned CLI, and maintain a read model parallel to it.
- `skills@1.7.4` is pinned; CLI output is not treated as a stable API.
- Every mutation is serialized, snapshotted, and verified; the CLI's exit code alone is never trusted.

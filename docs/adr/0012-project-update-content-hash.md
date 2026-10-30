# Project update checks compare archive content fingerprints

Status: accepted

Global update checks compare lock v3 `skillFolderHash` — a 40-character Git tree SHA — against the GitHub Trees API, and the checker explicitly rejects 64-character SHA-256 fingerprints. Project locks (`skills-lock.json`, v1) store `computedHash`, the CLI's SHA-256 over the skill folder (sorted relative paths, then file contents, excluding `.git` and `node_modules`), so the tree-SHA path cannot be reused; the CLI has no check-only mode either (`skills check` installs updates). Project scopes therefore get their own read-only check path: group lock entries by (source, ref), resolve the latest commit, download that source archive once per commit (cached), compute each skill folder's SHA-256 with the CLI-identical algorithm, and compare it with `computedHash`. Both check paths still resolve sources through the shared `update_target` resolver (ADR 0009) and keep the check/execution split; only the fingerprint domain and remote material differ. Global scope keeps the existing tree-SHA path.

## Considered options

- **Reuse tree SHA comparisons**: impossible, the two fingerprints are in different domains.
- **No update detection for projects**: rejected because it breaks lifecycle parity and pushes users back to the CLI.
- **Record app-side tree-SHA baselines per skill**: rejected because it conflicts with "filesystem plus lock are the only truth", breaks for installs made outside the app, and needs drift handling for every lock change.
- **Run the CLI update/check command**: rejected because it rewrites attachments, has no dry run, and overwrites local changes.

## Consequences

- A project check costs one archive download per source per new commit, cached by commit; per-scope checking (six-hour staleness) bounds the cost.
- Only public GitHub sources are checkable; other entry types show "unsupported", matching global behavior.
- Rust must mirror the CLI folder-hash algorithm exactly and stay pinned to the vendored CLI version; parity is enforced with golden vectors.
- The existing Git tree implementation in `git_tree.rs` is untouched; the content-hash path adds its own walker and archive cache.
- Project lock entries carry no install/update timestamps, so the UI shows "—" for those fields.
- The hash walker only orders printable ASCII paths. A skill folder containing a non-ASCII relative path is reported unsupported ("无法检测") instead of being hashed with a guessed collation order: a wrong order would produce a false "update available". Projects whose skill contents use non-ASCII file names therefore never report Current/Update until the CLI itself changes its ordering rule. This is an accepted limitation, not an ICU work item.

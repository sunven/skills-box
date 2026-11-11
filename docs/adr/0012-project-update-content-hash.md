# Project update checks compare archive content fingerprints

Status: accepted

On 2026-11-11, compatibility was extended to project lock v2, introduced by `skills@1.8.1`. Reading a v2 lock must preserve it and select its portable content fingerprint; accepting the version while retaining the v1 algorithm would report false Update candidates. V1 locks retain the legacy algorithm. V2 sorts relative paths by UTF-8 bytes, hashes `path NUL byte-length NUL content`, normalizes CRLF to LF for content without NUL bytes, excludes `metadata.json` entries and `.git`, `__pycache__`, and `__pypackages__` directories, and follows links contained within the Skill while skipping cycles. It includes `node_modules`. Source archives retain the files and links needed by either version and apply each Skill's exclusions and link boundary during evaluation; the archive cache version advances to invalidate older filtered archives. Copy to project computes its new Lock entry with the destination lock's algorithm, preserves that version, and rejects confirmation if the version changes. These rules supersede the v1-only details below; the read-only check boundary remains the same.

Global update checks compare lock v3 `skillFolderHash` — a 40-character Git tree SHA — against the GitHub Trees API, and the checker explicitly rejects 64-character SHA-256 fingerprints. Project locks (`skills-lock.json`, v1) store `computedHash`, the CLI's SHA-256 over the skill folder (sorted relative paths, then file contents, excluding `.git` and `node_modules`), so the tree-SHA path cannot be reused; the CLI has no check-only mode either (`skills check` installs updates). Project scopes therefore get their own read-only check path: group lock entries by (source, ref), resolve the latest commit, download that source archive once per commit (cached), compute each skill folder's SHA-256 with the CLI-identical algorithm, and compare it with `computedHash`. Both check paths still resolve sources through the shared `update_target` resolver (ADR 0009) and keep the check/execution split; only the fingerprint domain and remote material differ. Global scope keeps the existing tree-SHA path.

## Considered options

- **Reuse tree SHA comparisons**: impossible, the two fingerprints are in different domains.
- **No update detection for projects**: rejected because it breaks lifecycle parity and pushes users back to the CLI.
- **Record app-side tree-SHA baselines per skill**: rejected because it conflicts with "filesystem plus lock are the only truth", breaks for installs made outside the app, and needs drift handling for every lock change.
- **Run the CLI update/check command**: rejected because it rewrites attachments, has no dry run, and overwrites local changes.

## Consequences

- A project check costs one archive download per source per new commit, cached by commit; per-scope checking (six-hour staleness) bounds the cost.
- Public HTTPS Git sources are checkable. GitHub keeps the API/codeload acquisition path; generic Git resolves a commit and shallowly fetches it into a temporary checkout through the existing process executor. The checkout is packaged with the CLI's exclusions, including files marked `export-ignore`, because install clones rather than exports it. The content evaluator is shared by project checks and global generic Git entries, whose lock v3 hashes are also SHA-256. GitHub global entries keep their tree-SHA check.
- Archive caches are separated by acquisition method and shared by commit within each method. Dropping a check cancels the acquisition worker, terminates its process group and cleans up its temporary checkout. Git acquisition probes Git independently of Node/npx; read-only checks do not require the CLI toolchain.
- Rust must mirror the CLI folder-hash algorithm exactly; parity is enforced with golden vectors derived from the vendored CLI version. As recorded in ADR-0001, CLI execution now follows `skills@latest`, so upstream hash changes require corresponding compatibility updates.
- The existing Git tree implementation in `git_tree.rs` is untouched; the content-hash path adds its own walker and archive cache.
- Project lock entries carry no install/update timestamps, so the UI shows "—" for those fields.
- The v1 hash walker only orders printable ASCII paths. A skill folder containing a non-ASCII relative path is reported unsupported ("无法检测") instead of being hashed with a guessed collation order: a wrong order would produce a false "update available". This remains an accepted limitation for v1, not an ICU work item. V2 uses the CLI's UTF-8 byte ordering and supports non-ASCII file names.

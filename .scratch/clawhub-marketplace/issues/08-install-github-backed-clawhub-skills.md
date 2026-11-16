# 08: Install GitHub-backed ClawHub skills at pinned commit

**What to build:** GitHub-backed ClawHub entries install at the resolver's fixed commit and subpath. The confirmation shows the repository, commit and subpath, the installation record keeps enough diagnostic reference data, and verification covers the pinned reference where the upstream provides content hashes. No Git installation identity, Git lock entry or Discovery source is created. If the gate did not prove that this kind can be pinned, this ticket records the native-adapter decision instead of shipping.

**Blocked by:** 02, 07

**Status:** wontfix

- [ ] The resolver result (repository, subpath, commit) is authoritative and is used for installation and confirmation.
- [ ] The installed content is verified against the pinned reference or content hash where available.
- [ ] The ClawHub record stores the resolved reference and diagnostics without changing the Git identity model.
- [ ] No Git lock entry, Git update target or Discovery source is produced.
- [ ] A failed resolver, ambiguous owner or mismatched content blocks the install with a clear error and retry path.
- [ ] If pinning is unsupported, the ticket stops implementation and records the native-adapter decision in the feature documents.
- [ ] Tests cover resolver success, mismatch and failure paths.

## Comments

- 2026-11-16: Stopped as the ticket requires. Explicit GitHub-backed pinning is unsupported: the resolver returns `repo/path/commit/contentHash`, but `clawhub install @owner/slug --version <commit>` exits 1 with `Version not found`, and without `--version` the CLI resolves the current latest commit. Sampled GitHub-backed entries are also pending-scan suspicious and require the forbidden `--force`. Pinning was not shipped, so the native HTTP+ZIP adapter decision is recorded instead: installation stays paused and a native adapter requires a new ADR. Evidence: [02-clawhub-cli-feasibility.md](../research/02-clawhub-cli-feasibility.md).

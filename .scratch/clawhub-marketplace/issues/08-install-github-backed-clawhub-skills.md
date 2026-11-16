# 08: Install GitHub-backed ClawHub skills at pinned commit

**What to build:** GitHub-backed ClawHub entries install at the resolver's fixed commit and subpath. The confirmation shows the repository, commit and subpath, the installation record keeps enough diagnostic reference data, and verification covers the pinned reference where the upstream provides content hashes. No Git installation identity, Git lock entry or Discovery source is created. If the gate did not prove that this kind can be pinned, this ticket records the native-adapter decision instead of shipping.

**Blocked by:** 02, 07

**Status:** ready-for-agent

- [ ] The resolver result (repository, subpath, commit) is authoritative and is used for installation and confirmation.
- [ ] The installed content is verified against the pinned reference or content hash where available.
- [ ] The ClawHub record stores the resolved reference and diagnostics without changing the Git identity model.
- [ ] No Git lock entry, Git update target or Discovery source is produced.
- [ ] A failed resolver, ambiguous owner or mismatched content blocks the install with a clear error and retry path.
- [ ] If pinning is unsupported, the ticket stops implementation and records the native-adapter decision in the feature documents.
- [ ] Tests cover resolver success, mismatch and failure paths.

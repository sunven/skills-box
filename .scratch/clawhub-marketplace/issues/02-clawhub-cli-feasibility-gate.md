# 02: ClawHub CLI feasibility gate

**What to build:** A reproducible, isolated verification of the official ClawHub CLI and read API behaviors that gate installation. The deliverable is evidence plus an explicit go/no-go recorded in the feature's research and specification. Cover archive and GitHub-backed references, explicit version/commit pinning, staged layout, non-interactive behavior, suspicious-skill blocking, exit and stderr shape, content verification against version file hashes, and rate-limit responses. If the gate fails, installation slices do not start and a native HTTP plus ZIP adapter is evaluated as a separate decision.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Archive fixture: an explicitly requested version is installed, and the staged folder plus `SKILL.md` prove that version rather than `latest`.
- [ ] GitHub-backed fixture: the resolver's repository, subpath and commit are recorded, and explicit reference pinning is either proven or explicitly proven unsupported.
- [ ] A suspicious skill is blocked without `--force`/`--force-install` in non-interactive mode; stdout, stderr and exit behavior are recorded.
- [ ] Installed content can be checked against the version's file hashes when available; the verification method and its limits are documented.
- [ ] 429/`Retry-After` behavior is observed and documented.
- [ ] A go/no-go outcome is appended to the feature research and specification; a no-go stops installation tickets and records the native-adapter option.

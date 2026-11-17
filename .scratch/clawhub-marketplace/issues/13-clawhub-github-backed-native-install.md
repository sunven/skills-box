# 13: GitHub-backed ClawHub install gate and native implementation

**What to build:** GitHub-backed ClawHub entries either install natively at the resolver's fixed commit or remain explicitly unsupported. A gate must first reproduce ClawHub's folder content hash from a pinned GitHub codeload archive and match the resolver's `contentHash` for sampled entries. If the gate passes, add codeload download at the exact commit, subpath extraction, hash verification, commit references and diagnostics to the existing install flow. If it fails, record the evidence, keep GitHub-backed installation blocked and state the limitation in UI and feature docs. There is no CLI fallback and no silent archive-only claim.

**Blocked by:** 11, 12

**Status:** resolved

- [ ] Parse resolver `repo/path/commit/contentHash` strictly; an ambiguous owner, missing or invalid commit, invalid path or unreadable resolver blocks with a clear error, canonical link and retry.
- [ ] Gate: for sampled resolver fixtures, download `https://codeload.github.com/{repo}/zip/{commit}`, extract the resolver's `path`, reproduce ClawHub's folder content hash exactly and match `contentHash`; record the algorithm, fixtures and evidence in a research file.
- [ ] If the gate fails, record the failure in the feature documents, mark GitHub-backed installation unsupported in the detail UI and close this ticket without shipping any fallback.
- [ ] If the gate passes: download with bounded size, extract only the pinned subpath, apply the same unsafe-entry and package-bound rules as ticket 11, verify the folder content hash, and publish through ticket 12's native flow.
- [ ] The Market record uses the 40-character commit as its reference and stores `repo/path/commit/contentHash` diagnostics; update checks compare commits, and no Git installation identity, Git lock entry or Discovery source is created.
- [ ] Moderation is enforced before installation: pending-scan, suspicious or malware entries stay blocked with the canonical link; no force and no scan bypass.
- [ ] Confirmation shows `repo@commit:path`, the commit reference, scope, Agents and the content caveat.
- [ ] Tests cover resolver failure, download failure, hash mismatch, unsafe archive entries, successful installation and the blocked-gate path.

## Comments

- 2026-11-17: Drafted with ADR-0035. Ticket 02 proved `clawhub install --version <commit>` fails; native codeload plus resolver verification is the only path that can pin this entry class. The undocumented content-hash scheme stays a hard gate.
- 2026-11-17: Resolved; gate PASS. Recovered the registry algorithm from `openclaw/clawhub` (`unzipToEntries` → `stripGitHubZipRoot` → `computeGitHubSkillFolderContentHash`), including JS `localeCompare` ordering; ICU4X en-US collation matches Node ICU 78.2 for representative paths. Recomputing `@aws/agents-build` from the pinned codeload archive matched `d79709bf46ca8cd0fffe392a914cd56539b4b7c0ff0fe34c9583f24057300195`, and 6 sampled folders from the same commit matched. Implemented `marketplace/clawhub/github.rs` (bounded codeload download, path normalization, Mac-junk filtering, common-root stripping, subpath extraction, hash verification, raw `SKILL.md` preview read), wired it into target resolution, staging, records with `repo/path/commit/contentHash` diagnostics, and the install UI. Moderation still blocks suspicious/pending-scan entries with no force path. The ignored live test resolves and stages `@aws/agents-build` successfully; see [03-github-content-hash-gate.md](../research/03-github-content-hash-gate.md). Full suites were re-run after the change.

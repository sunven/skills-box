# 11: Native ClawHub archive package fetch, extraction and verification

**What to build:** A source-specific staging adapter that turns one archive-hosted ClawHub coordinate and an explicit semver reference into a verified skill folder in an isolated temporary directory. It downloads the registry ZIP anonymously, extracts it without touching the Canonical store or any Agent directory, and proves the extracted content against the version file list before any publication can consume it. This ticket adds no UI and starts no Mutation session.

**Blocked by:** None (ADR-0035 accepted)

**Status:** resolved

- [ ] Download `GET https://clawhub.ai/api/v1/download?slug=…&ownerHandle=…&version=…` anonymously over HTTPS under the existing ClawHub client policy: direct connection with host proxies disabled, bounded timeout, bounded response body, no account or token, and respect for the shared 429/`Retry-After` cooldown.
- [ ] Extract into a fresh temporary staging directory; reject path traversal, absolute paths, duplicate paths, case-insensitive collisions, symlinks, special files, encrypted entries and any entry that would escape the staging root.
- [ ] Enforce package bounds before and during extraction: maximum compressed response size, maximum total uncompressed size, maximum entry count, maximum single-file size and a compression-ratio guard; fail without writing outside staging.
- [ ] Fetch `GET /api/v1/skills/{slug}/versions/{version}?ownerHandle=…` and verify the exact content: every listed file exists as a regular file with matching size and SHA-256; the only allowed extra is the registry-generated `_meta.json` whose version equals the requested version; any other extra, including `.clawhub/`, is rejected.
- [ ] Parse the original skill name from `SKILL.md` with the existing marketplace rule and fail when it is missing or invalid.
- [ ] Return a staging handle containing the verified folder path and observed content hash; clean up partial staging on every error and never write into the canonical folder or Agent directories.
- [ ] Tests use a loopback contract server and generated ZIP fixtures: success, hash mismatch, size mismatch, missing file, extra file, `_meta.json` version mismatch, symlink, traversal, duplicate paths, compression bomb, oversized body, truncated download and 429.
- [ ] One ignored live test downloads an explicit archive version twice and compares per-file hashes without publishing; it never touches the real user environment.

## Comments

- 2026-11-17: Drafted with ADR-0035. Ticket 02 proved the official CLI cannot pin GitHub-backed entries and blocks pending-scan entries; this adapter replaces CLI staging for archive entries while keeping the Marketplace record identity and publication pipeline.
- 2026-11-17: Resolved. Added `marketplace/clawhub/package.rs` (`stage_archive`, `StagedPackage`) with an anonymous bounded download, two-pass ZIP inspection, exact file-set and per-file SHA-256 verification against the version file list, `_meta.json` version check, and staging-only extraction with path, duplicate, case-collision, symlink, entry-count, size and compression-ratio guards. The ClawHub test base override is now serialized and resets cache/cooldown per local-server test. Local contract tests cover success, hash/size/missing/extra/`.clawhub`/meta mismatches, traversal, absolute paths, duplicates, case collisions, unexpected directories, symlinks, compression bombs, oversized and truncated downloads, and 429; the ignored live test passed against `@steipete/weather@1.0.0`. Full Rust suite: 792 passed, 0 failed (two host-sensitive tests skipped as documented).

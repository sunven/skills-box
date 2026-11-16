# ClawHub CLI feasibility gate (ticket 02)

Checked: 2026-11-16. Scope: the official `clawhub` CLI v0.26.0 and anonymous read API, exercised in an isolated temp environment. This document records evidence and the GO/NO-GO decision. No product code was changed.

## Verdict

**NO-GO for the CLI-based ClawHub installation channel as specified.** The hard gate is explicit reference pinning for both entry classes. Archive pinning works and archive content is independently verifiable, but the CLI cannot install a GitHub-backed entry at a caller-supplied commit: `--version <commit>` fails with `Version not found`, and there is no other pinning flag or ref form. GitHub-backed entries are also returned by the registry as `moderation.isSuspicious: true` in the sampled fixtures (pending scan), so a non-interactive install without `--force` is blocked, and the specification forbids automatic `--force`/`--force-install`.

Per the specification and ADR-0034, installation is paused and a native HTTP+ZIP adapter must be evaluated as a separate decision with a new ADR. Archive-only installation is technically viable but is a narrower scope than the approved specification; it must not be adopted silently. Browse/detail tickets (04/05/06) are unaffected because the anonymous read API works. Installation tickets 07/08/09 must not start.

## Environment and isolation

| Item | Value |
| --- | --- |
| OS | macOS (darwin 27.2.0), arm64 |
| Node / npm | v24.15.0 / 11.12.1 |
| CLI | `clawhub@0.26.0`, build label `123d0f6c` (`clawhub -V` prints `0.26.0`) |
| Install method | `npm install --prefix "$TMP/prefix" clawhub@0.26.0` (54 packages, no `-g`) |
| Isolation | `HOME=$TMP/home`, `XDG_*` under `$TMP`, npm cache/prefix under `$TMP`, `--workdir`/`--dir` under `$TMP`; stdin from `/dev/null`, stdout/stderr redirected (no TTY) |
| Upstream volume | 34 ClawHub API/download requests plus 1 GitHub codeload ZIP, counted from the probe sequence (22 direct read GETs, 1 direct ZIP download, and 11 CLI-initiated requests); no retries against upstream; 429 was tested against a local mock |

`$TMP` is a disposable temp root. All paths below are redacted to `$TMP`; the real user HOME, global npm state, and `~/.clawhub` were never used. The CLI's macOS config path is `$HOME/Library/Application Support/clawhub/config.json`, so the isolated HOME also isolated config.

CLI installation check:

```
$ "$TMP/prefix/node_modules/.bin/clawhub" -V
0.26.0
$ "$TMP/prefix/node_modules/.bin/clawhub" --version
error: unknown option '--version'
```

`--version` is not a valid global flag; dependency detection must use `-V`/`--cli-version` and parse the plain version line.

## Gate results

| # | Gate requirement | Result | Evidence |
| --- | --- | --- | --- |
| 1 | Archive: explicitly requested version is installed, not `latest` | **PASS** | Probe 1: `@steipete/nano-banana-pro@1.0.0` staged with `v1.0.0` SKILL.md hash; latest is `1.0.1` with a different SKILL.md |
| 2 | GitHub-backed: explicit reference pinning honored or proven unsupported | **FAIL (proven unsupported)** | Probe 2: resolver returned `repo/path/commit`; `--version <commit>` exits 1 with `Version not found`; no alternative flag exists |
| 3 | Suspicious skill blocked without `--force`/`--force-install` in non-interactive mode | **PASS (block works), policy conflict remains** | Probe 3: real `moderation.isSuspicious` skill exits 1 with plain-text error and no staged files; `--force-install` alone does not bypass; GitHub-backed pending-scan entries require the forbidden `--force` to install |
| 4 | Installed content verifiable against version file hashes where available | **PASS with limits** | Probe 4: archive SKILL.md/script hashes match `/versions/1.0.0`; GitHub folder content hash matches resolver `contentHash`; download has no digest; generated `_meta.json` and `.clawhub/` are outside the version file list |
| 5 | 429/`Retry-After` behavior observed and documented | **PASS (simulated), real upstream unobserved** | Probe 5: local 429 with `Retry-After: 1` produced 3 attempts in 2.67 s, then exit 1 with a retry/reset diagnostic; documented upstream limits recorded |
| 6 | Catalog composition informs tickets 04/05 | **Informational** | Probe 6: `/api/v1/skills` has no `install`/`kind` field in 128 sampled items; `/api/v1/search` exposes `install.kind` (`clawhub`, `skills-sh`) |
| 7 | Exit/stderr shape for ordinary failures | **PASS** | Probe 7: nonexistent skill exits 1 with plain-text stderr, no JSON |
| 8 | Lock/origin fields relevant to version/identity, and their authority | **PASS (not authoritative)** | Probe 8: lock/origin contain version/slug/owner/fingerprint only; no upstream digest; GitHub record lacks repo/path; plain mutable JSON |

## Probe 1 — archive version pinning

`@steipete/weather` was the suggested fixture, but it currently has exactly one version, so it cannot prove "older than latest":

```
GET /api/v1/skills/weather/versions?ownerHandle=steipete
{"items":[{"version":"1.0.0",...}],"nextCursor":null}
GET /api/v1/skills/weather?ownerHandle=steipete
... "latestVersion":{"version":"1.0.0",...}
```

Fixture substituted: `@steipete/nano-banana-pro`, which has `1.0.1` (latest) and `1.0.0` (older). Command, with global flags before the subcommand and an absolute `--dir`:

```
"$TMP/prefix/node_modules/.bin/clawhub" \
  --workdir "$TMP/work/archive-pin" \
  --dir "$TMP/work/archive-pin/staged" \
  --no-input install @steipete/nano-banana-pro --version 1.0.0
```

Output (exit 0):

```
Installed nano-banana-pro v1.0.0 -> $TMP/work/archive-pin/staged/@steipete/nano-banana-pro
```

Staged layout:

```
$TMP/work/archive-pin/.clawhub/lock.json
$TMP/work/archive-pin/staged/@steipete/nano-banana-pro/_meta.json
$TMP/work/archive-pin/staged/@steipete/nano-banana-pro/.clawhub/origin.json
$TMP/work/archive-pin/staged/@steipete/nano-banana-pro/scripts/generate_image.py
$TMP/work/archive-pin/staged/@steipete/nano-banana-pro/SKILL.md
```

Proof that this is `1.0.0`, not latest `1.0.1`:

| File | Installed `1.0.0` | API `1.0.0` | API latest `1.0.1` |
| --- | --- | --- | --- |
| `SKILL.md` | sha256 `bf123f49...8ad1ec7`, 1083 bytes | same sha256, 1083 bytes | sha256 `4ea88f4b...a7988b`, 5701 bytes |
| `scripts/generate_image.py` | sha256 `8ecca97d...2023e8`, 5613 bytes | same | same |
| `skill-card.md` | absent | absent | present |

The installed file set matches the `1.0.0` version file list plus the registry-generated `_meta.json`; the `1.0.1` file set is different. `_meta.json` also says `"version":"1.0.0"`.

### Path and flag-order behavior

- Native ref `@owner/slug` stages to `<dir>/@owner/slug`, not `<dir>/<slug>`. This corrects the earlier contract note. Lock keys are also `@owner/slug`.
- `--dir` accepts an absolute path and wins over `--workdir`. A failing install with `--workdir $TMP/work/abs-dir-workdir --dir $TMP/work/abs-dir-outside/staged` created `$TMP/work/abs-dir-outside/staged` and nothing in the workdir.
- Relative `--dir` (default `skills`) resolves under `--workdir`; the GitHub probe used this and produced `$TMP/work/github-fixture/skills/@aws/agents-build`.
- Global flags `--workdir` and `--dir` are honored after the subcommand too: `clawhub --no-input install <ref> --workdir ... --dir ...` created the requested directory. Both orders are usable, but putting global flags before `install` is the documented form and is safer.
- `--version` is an install-command option and works after the skill ref, as shown.

## Probe 2 — GitHub-backed resolution and pinning

Resolver data:

```
GET /api/v1/skills/agents-build/install?ownerHandle=aws
{"ok":true,"slug":"agents-build","installKind":"github",
 "github":{"repo":"aws/agent-toolkit-for-aws",
           "path":"plugins/aws-agents/skills/agents-build",
           "commit":"b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c",
           "contentHash":"d79709bf46ca8cd0fffe392a914cd56539b4b7c0ff0fe34c9583f24057300195",
           "sourceUrl":"https://github.com/aws/agent-toolkit-for-aws/tree/b4416ddd.../plugins/aws-agents/skills/agents-build"}}
```

The same commit is published by `GET /api/v1/feeds/skills` (1000 entries: 477 `public-clawhub`, 523 `public-github`). Version list is empty and detail `latestVersion` is `null`, so `/install` is the resolver.

Without `--force`, this fixture is blocked as suspicious (Probe 3). To continue the pinning probe, `--force` was used **only in the isolated probe**:

```
clawhub --workdir "$TMP/work/github-fixture" --no-input install @aws/agents-build --force
# exit 0
Installed agents-build b4416dddac9b -> $TMP/work/github-fixture/skills/@aws/agents-build
```

Attempting to pass the resolver commit explicitly:

```
clawhub --workdir "$TMP/work/github-pin" --no-input \
  install @aws/agents-build --version b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c --force
# exit 1
Version not found
Error: Version not found
```

No target folder or lock was created. Source inspection confirms why: with any `versionFlag`, the CLI skips `resolveLatestSkillInstall` entirely, validates the value through `/api/v1/skills/{slug}/versions/{version}`, and always downloads an archive at that version. `installKind=github` is only reachable when no version is requested. `skills-sh:` refs have their own early rejection:

```
clawhub --no-input install skills-sh:aws/agent-toolkit-for-aws/agents-build --version b4416ddd...
Error: --version is not supported for skills.sh catalog references
```

**Conclusion: explicit GitHub-backed pinning is unsupported.** Without `--version`, the CLI resolves and downloads whatever commit is latest at invocation time; the caller cannot force the commit shown earlier in the UI or confirmation. The app can compare the resolver commit with the lock/origin commit after the fact, but it cannot prevent a re-resolution to a different commit, so this does not satisfy the hard gate.

### GitHub content verification (independent of the CLI)

The installed folder was hashed with the CLI's own `listSkillFiles` plus `buildGitHubFolderContentHash` (the same scheme the resolver uses):

```
contentHash = d79709bf46ca8cd0fffe392a914cd56539b4b7c0ff0fe34c9583f24057300195
```

This matches the resolver and feed `contentHash`. The native GitHub install path does **not** pass `expectedContentHash` (only the `skills-sh:` path does), so the CLI itself does not verify it; the app must verify it after staging if this path is ever authorized.

## Probe 3 — suspicious-skill blocking

Fixture `@aws/agents-build` has skill-level `moderation.isSuspicious: true` (`verdict: "suspicious"`, empty reason codes, null engine/updatedAt — consistent with a pending scan). Non-interactive install with no flags:

```
clawhub --workdir "$TMP/work/github-fixture" --no-input install @aws/agents-build
# exit 1
stdout: [warning] Warning: "agents-build" is flagged for ClawHub security review.
        This skill may contain risky patterns ...
stderr: Error: Use --force to install suspicious skills in non-interactive mode
```

Afterward `$TMP/work/github-fixture` contained only an empty `skills/` directory: no target folder, no `.clawhub/lock.json`, no partial content. `--force-install` alone produced the identical warning and error, so it does not bypass the moderation gate:

```
clawhub ... install @aws/agents-build --force-install
# exit 1, same stderr
```

With a pre-existing target, the CLI refuses even earlier and leaves the sentinel untouched (sha256 unchanged before/after):

```
Error: Already installed: $TMP/work/existing-target/skills/@aws/agents-build (use --force)
```

Source confirms two important constraints:

- The suspicious prompt path uses `isInteractive()` (`stdout.isTTY && stdin.isTTY`), not `--no-input`. In a real TTY, `--no-input` would not suppress the prompt. The app must run the CLI with piped/non-TTY stdio, not rely on `--no-input` alone.
- The check reads skill-level `moderation`; version-level `security` is not consulted during install. For example, `nano-banana-pro@1.0.1` reports version `security.status: "suspicious"` while skill moderation is clean, and the install code would not block it. This is source inspection, not an execution test.

Both GitHub-backed fixtures sampled (AWS and NVIDIA) had `moderation.isSuspicious: true`. The registry exposes a `--force-install` flag described as "Install a pending GitHub-backed skill before ClawHub scan completes", which supports the pending-scan interpretation. This was not exhaustively checked across all 523 GitHub feed entries.

## Probe 4 — archive content verification

`GET /api/v1/skills/nano-banana-pro/versions/1.0.0?ownerHandle=steipete` returns `version.files[]` with `path`, `size`, `sha256`. Method: after CLI staging, hash each regular file (excluding `.clawhub/`) with SHA-256 and compare to the version's file list. For `1.0.0`:

- `SKILL.md`: `bf123f49...` = API value, 1083 bytes.
- `scripts/generate_image.py`: `8ecca97d...` = API value, 5613 bytes.
- `_meta.json` is present in the ZIP and staged folder but is **not** in the version file list; it is registry-generated (`ownerId`, `slug`, `version`, `publishedAt`). Verification must treat it as an allowed extra, or verify its `version` field separately.
- `.clawhub/origin.json` is created by the CLI after extraction and is not part of the package content.

The ZIP download has no digest:

```
GET /api/v1/download?slug=nano-banana-pro&ownerHandle=steipete&version=1.0.0
HTTP/2 200
content-type: application/zip
cache-control: private, no-store
(no Digest, no ETag, no Content-MD5)
zip sha256 = ba087d03610a92f555fe49eafe8936c09d614ee0e4c961c2a816918ee4d3eb38
```

Source inspection confirms the CLI fetches the ZIP as raw bytes with no hash comparison for archive installs. Per-file API hashes are therefore the only available post-install verification, and they cannot prove the ZIP itself was not modified before extraction, nor that the same version will have identical content later (no immutability contract).

## Probe 5 — 429 / Retry-After

No real upstream 429 was triggered (deliberately; 20 total upstream reads/downloads). Behavior was observed against a local mock registry via `--registry http://127.0.0.1:<port>`, which returned HTTP 429 with `Retry-After: 1`, `x-ratelimit-limit: 3000`, `x-ratelimit-remaining: 0`:

```
clawhub --registry http://127.0.0.1:<port> --workdir "$TMP/work/rate-limit" --no-input \
  install @mock/rate-limited-skill
# exit 1, elapsed 2.67 s, mock received 3 GETs:
# GET /api/v1/skills/rate-limited-skill?ownerHandle=mock   (x3)
stderr:
{"error": "rate limited", "code": "RATE_LIMITED"} (retry in 1s, remaining: 0/3000, reset in 1s)
Error: {"error": "rate limited", "code": "RATE_LIMITED"} (retry in 1s, remaining: 0/3000, reset in 1s)
```

So the CLI performs 2 retries (3 attempts total), honors `Retry-After` with jitter, surfaces remaining/reset values, and then exits 1 with plain text. Source: `RETRY_COUNT = 2`, retries on 408/429/5xx, `Retry-After` honored up to 60 s (longer waits fail fast), and the same retry wrapper covers binary downloads. Documented upstream limits remain 3000 reads/min/IP and 1200 downloads/min/IP; normal responses in this session carried no `x-ratelimit-*` headers.

## Probe 6 — catalog composition

List endpoint samples (3 pages, 128 items total):

```
GET /api/v1/skills?limit=50&sort=downloads   -> 50 items, 0 with latestVersion null
GET /api/v1/skills?limit=50&sort=updated     -> 50 items, 0 with latestVersion null
GET /api/v1/skills?prefix=debug&limit=50     -> 28 items, 1 with latestVersion null
item keys: createdAt, description, displayName, latestVersion, metadata,
           ownerHandle, slug, stats, summary, tags, topics, updatedAt
```

There is **no `install` or `kind` field** on list items. A targeted list query for the sampled alias did not return it: `anthropics/debug` was absent from `prefix=debug`, while search returned it as a `skills-sh` entry. This is consistent with the list containing native ClawHub entries only, but 128 samples cannot prove total exclusion; list items cannot be classified as aliases at all without a kind field.

Search endpoint samples:

```
GET /api/v1/search?q=weather&limit=20         -> 20 clawhub
GET /api/v1/search?q=design&limit=20          -> 20 clawhub
GET /api/v1/search?q=anthropic&limit=20       -> 20 clawhub
GET /api/v1/search?q=knowledge-work&limit=20  -> 14 skills-sh + 6 clawhub
```

A `skills-sh` result carries `install.kind: "skills-sh"`, `install.reference: "skills-sh:anthropics/knowledge-work-plugins/debug"`, `install.sourceUrl`, `source: "skills-sh"`, `native: null`, and `trust.sourceFreshness: "observed-only"`. A native result carries `install.kind: "clawhub"` and `install.reference: "owner/slug"`.

Implication for tickets 04/05: filter aliases on search using `install.kind`; the list page cannot identify them and must follow the specification's fallback (show normally; identify in detail and redirect to skills.sh). Do not call the resolver per list item.

## Probe 7 — ordinary failure shape

```
clawhub --no-input install @steipete/definitely-not-a-skill-20261116 --workdir ... --dir ...
# exit 1
stderr:
Skill not found or unavailable to this account.
Error: Skill not found or unavailable to this account.
```

No JSON mode (`install --help` has no `--json`); stdout is empty. The message appears twice because the spinner error handler prints it and the thrown error is then printed as `Error: ...`. On success stdout is a single human line (`Installed <slug> v<version> -> <path>`); control characters/spinner output depend on TTY. Implementers must treat exit code plus staged-content verification as the result, and show a stderr summary rather than parse a machine format.

## Probe 8 — lock and origin files

Archive install:

```
$TMP/work/archive-pin/.clawhub/lock.json
{"version":1,"skills":{"@steipete/nano-banana-pro":{
  "version":"1.0.0","installedAt":1794800465180,"ownerHandle":"steipete"}}}

$TMP/work/archive-pin/staged/@steipete/nano-banana-pro/.clawhub/origin.json
{"version":1,"registry":"https://clawhub.ai","slug":"nano-banana-pro",
 "ownerHandle":"steipete","installedVersion":"1.0.0",
 "installedAt":1794800465180,
 "fingerprint":"4fb52f0086c815b1a01b1b5c0b46f943e1adb9d6cf00f81d69a51ce9a05bdc3a"}
```

GitHub-backed install:

```
lock:   {"version":1,"skills":{"@aws/agents-build":{
          "version":"b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c",
          "installedAt":1794800617783,"ownerHandle":"aws"}}}
origin: {"version":1,"registry":"https://clawhub.ai","slug":"agents-build",
         "ownerHandle":"aws",
         "installedVersion":"b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c",
         "installedAt":1794800617783,
         "fingerprint":"1fa439669c1f916a06c7ea897e192f8e423d8b273aea18b4eea283a7a574c363"}
```

Findings:

- The archive lock records only the semver string, owner, and timestamp. There is no content digest, source URL, repository, or commit. The origin fingerprint is a locally computed hash over installed files (recomputed and matched: `4fb52f00...`), not an upstream digest; anyone can recompute it after editing content.
- The native GitHub record stores the commit only as `version`/`installedVersion`. It has no `repo`, `path`, `sourceUrl`, or `contentHash`, so it cannot reconstruct the resolver provenance by itself; that must come from the app's own resolver call.
- The lock schema has optional `sourceRef`, `sourceRepository`, `sourcePath`, `sourceUrl`, `canonicalRef`, `clawhubScan`, and `trustLabel` fields, but they are written for `skills-sh:` installs, not native ClawHub installs.
- Both files are plain, mutable JSON with no signature. They are useful as CLI-local state and local-change fingerprints, but must not be the sole identity or version authority. The app must persist the resolver reference and its own content baseline, and should not treat the lock's version as proof that the requested content was installed.

## Deviations and limitations

- `@steipete/weather` has only `1.0.0`; the ticket's suggested older-version fixture was replaced by `@steipete/nano-banana-pro` (`1.0.0` vs latest `1.0.1`).
- `clawhub --version` is not a valid flag; version detection requires `-V`.
- Real upstream 429 was not observed; the CLI's behavior was proven with a local mock. Upstream limits are documentation values.
- Suspicious blocking was proven with GitHub-backed pending-scan skills; an archive skill with skill-level `isSuspicious` was not found within the small sample. The code path is class-independent and was confirmed in source.
- GitHub content-hash verification matched for one fixture; it was not exercised across many entries.
- Alias exclusion from `/api/v1/skills` is inferred from the absence of `install.kind`, 128 sampled items, and one targeted alias lookup. It is not a contract.
- Version pinning and content stability remain upstream-uncontracted; even the archive pass is a point-in-time result for CLI 0.26.0.

## Constraints implementers must honor

Until a new decision changes the scope:

1. Do not start installation tickets 07/08/09. Browse/detail work may proceed against the anonymous API; do not route installs through Git or silently narrow to archive-only.
2. Do not pass `--force` or `--force-install` automatically. `--force-install` does not bypass the suspicious gate; `--force` does, and is forbidden.
3. Run any future CLI invocation with non-TTY stdio (pipes), an isolated HOME, explicit `--workdir`, and an absolute `--dir`. `--no-input` alone does not suppress the suspicious prompt in a TTY.
4. Use `-V` (not `--version`) for CLI detection and pin `clawhub@0.26.0`; never auto-update or install globally.
5. Native refs stage to `<dir>/@owner/<slug>`; lock keys are `@owner/slug`. Publication must use that folder, and strip `.clawhub/` before publishing.
6. Archive installs must pass `--version`, then verify staged files against `/api/v1/skills/{slug}/versions/{version}` `files[].sha256`; treat `_meta.json` and `.clawhub/` as extras. The ZIP download has no digest and the CLI does not verify archive hashes.
7. Do not claim GitHub-backed pinning. The CLI only resolves latest; `--version <commit>` fails. If a new adapter is authorized, capture resolver `repo/path/commit/contentHash` before install, compare the installed commit, and verify the folder content hash.
8. Treat lock/origin as CLI-local hints only. Persist the app's own resolved reference and content baseline; do not derive authoritative identity from lock/origin.
9. Handle failures by exit code plus stderr text; there is no JSON. Expect exit 1 for all failures and occasional duplicate message lines.
10. Honor 429/`Retry-After` at the app layer as well; the CLI retries twice and caps `Retry-After` at 60 s, but its retries do not replace app-level backoff and rate-limit isolation.
11. Filter `skills-sh` aliases only where `install.kind` exists (search); the list endpoint has no kind field. Follow the spec fallback for list/detail.

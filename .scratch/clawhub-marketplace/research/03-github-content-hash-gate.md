# GitHub-backed content-hash gate (ticket 13)

Date: 2026-11-17. Scope: determine whether Skills Box can reproduce ClawHub's `githubCurrentContentHash` from a pinned GitHub codeload archive. No product code is required for the gate itself.

## Verdict

**PASS.** The registry-side folder hash is reproducible from `codeload.github.com/{repo}/zip/{commit}` using a documented-in-source algorithm and ICU en-US collation. A pinned live sample matched exactly, and 6 sampled skill folders from the same repository matched. GitHub-backed native installation is therefore enabled; moderation still blocks suspicious or pending-scan entries.

## Algorithm (read from `openclaw/clawhub` @ `main`)

Sources:

- `convex/githubSkillSync.ts`: `fetchGitHubSkillSourceSnapshotWithEntries`, `unzipToEntries`, `stripGitHubZipRoot`, `listGitHubSkillFolderEntries`.
- `convex/lib/githubSkillSync.ts`: `computeGitHubSkillFolderContentHash`, `normalizeRepoPath`.
- `packages/clawhub/src/skills.ts`: `buildGitHubFolderContentHash` (same payload format for the CLI verifier).

Steps:

1. Download the repository ZIP from `codeload.github.com/{repo}/zip/{commit}`.
2. Unzip every regular file. Skip directory entries. Normalize paths: backslashes to `/`, trim, strip a leading `./` and leading `/`, drop empty segments, reject NUL/`.`/`..` segments. Skip Mac junk (`__MACOSX` segment, `.DS_Store`, `._*`). Limits: 80 MB total uncompressed, 10 MB per file, 75,000 entries.
3. Strip the single common archive root (GitHub's `{repo}-{commit}/`).
4. For the resolver's `path` folder, take each file's relative path, its byte size and SHA-256.
5. Sort by path using JavaScript `localeCompare` (default locale, `en-US` in the registry runtime).
6. Payload: `"{relativePath}\0{size}\0{lowercaseSha256}"` lines joined by `\n`, with no trailing newline; SHA-256 of the UTF-8 payload is `contentHash`.

Dotfiles are included; `.gitignore` is **not** applied server-side. The CLI's separate `listSkillFiles` verifier skips dotfiles and ignores files, so it is not the algorithm to reproduce for the resolver hash.

## Collation check

Node 24 (`ICU 78.2`, `en-US`) and ICU4X 2.4 `Collator` with `locale!("en-US")` produced identical orderings for representative paths including case, space, `_`, `-`, `.` and `/`:

```
a b, a_b, a-b, a.b, a/b, a/z, a/Z, a0, a9,
assets/a.png, assets/a/b.png, docs/index.md, readme.md, README.md,
references/guide.md, scripts/run.py, scripts/z.py, skill.md, SKILL.md
```

Byte-wise sorting does **not** match and must not be used.

## Live evidence

Pinned sample: `@aws/agents-build`

| Item | Value |
| --- | --- |
| Repo | `aws/agent-toolkit-for-aws` |
| Commit | `b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c` |
| Path | `plugins/aws-agents/skills/agents-build` |
| Resolver `contentHash` | `d79709bf46ca8cd0fffe392a914cd56539b4b7c0ff0fe34c9583f24057300195` |
| ZIP | 4.7 MB codeload archive |
| Recomputed hash | `d79709bf46ca8cd0fffe392a914cd56539b4b7c0ff0fe34c9583f24057300195` |

Additional paths sampled from the official feed's 522 GitHub-backed candidates in the same commit, all matched:

```
plugins/aws-agents/skills/agents-connect
plugins/aws-agents/skills/agents-debug
plugins/aws-agents/skills/agents-deploy
plugins/aws-agents/skills/agents-get-started
plugins/aws-agents-for-devsecops/skills/coordinating-multi-space-devops-agent
```

The ignored Rust test `clawhub_live_github_backed_stage_matches_the_resolver_hash` also passed: it resolves `@aws/agents-build` through the anonymous resolver, reads `SKILL.md` at the pinned commit, downloads and verifies the codeload archive, and stages the folder.

## Implications

- GitHub-backed installs download only the pinned commit, extract only the resolver's subpath, and fail closed on hash mismatch.
- Moderation remains a separate gate: `@aws/agents-build` is flagged pending-suspicious and stays uninstallable until upstream clears it. No `--force` equivalent exists.
- The resolver hash covers all files under the subpath, including dotfiles and symlink target text; symlink archive entries are extracted as regular files and never create symlinks.
- The hash depends on undocumented registry behavior. A new ClawHub algorithm or locale change must re-open this gate; the app never silently falls back to unverified content.

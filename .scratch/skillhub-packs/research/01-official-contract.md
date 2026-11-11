# SkillHub expert-pack contract: verified facts

Observed: **2026-11-11**, approximately 03:56–04:00 UTC / 12:56–13:00 Asia/Tokyo. Facts only; no product decision or implementation proposal. Read the repository glossary and ADR-0029/0030. External verification used read-only HTTP requests after `web.run` returned no content. Downloaded archives were inspected in temporary storage; no skill content or installer was executed and no real Agent directory was changed.

## Sources and evidence identity

- **S1 — Official pack installation instructions:** <https://skillhub.cn/install/skillhub-pack-install.md>.
- **S2 — Public website and its deployed JavaScript:** <https://skillhub.cn/skillspackage>, <https://skillhub.cn/skillspackage/hr-resume-screening>, <https://cloudcache.tencent-cloud.com/qcloud/tea/app/skillhub/assets/skill-hub.bb2n2rxv.js?max_age=31536000>. Function names below refer to this deployed bundle.
- **S3 — Public list/search:** <https://api.skillhub.cn/api/v1/skillsets?page=1&pageSize=200>.
- **S4 — Example detail:** <https://api.skillhub.cn/api/v1/skillsets/hr-resume-screening>.
- **S5 — Example backend ZIP:** <https://api.skillhub.cn/api/v1/skillsets/hr-resume-screening/download>.
- **S6 — Official CLI release:** <https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/version.json> identifies `2026.8.5` and <https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/releases/skillhub-cli-2026.8.5.tar.gz>. Downloaded tar SHA-256: `3bbe2ba15ada2eb7a94a2b760fead83be5f4164ab28a6c8b0944dbc539f7e236`. Its `cli/skills_store_cli.py` SHA-256 is `6c5c89142b32b92d212d7b86821ba1f3cc0d6dc68df6d901e0ef38ece4add797`, identical to the previously downloaded source inspected here. Relevant lines: 1174–1274 (member extraction), 1335–1356 (lock), 3682–3940 (pack install), 5518–5530 (parser), 5811–5836 (main dispatch).
- **S7 — Example member:** <https://api.skillhub.cn/api/v1/skills/easy-recruitment?namespace=clawhub_heyweekly>, <https://api.skillhub.cn/api/v1/download?slug=%40clawhub_heyweekly%2Feasy-recruitment>.

## Browse, search and identity

1. Unauthenticated `GET /api/v1/skillsets` returns `{skillSets: [...], total: number}`. Default request returned 20 rows and `total: 54`; `page=1&pageSize=200` returned all 54. `page=1&pageSize=2` and `page=2&pageSize=2` returned different pairs. [S3]
2. Website `fetchSkillSets` sends `page`, `pageSize`, optional trimmed `keyword`, and optional `scene`. Verified `keyword=hr-resume-screening` returns one pack; `keyword=简历` returns two; `scene=hr` returns one. The current website fetches all pages at `pageSize=200`, searches remotely, and applies its scene filter locally. Supported server-side `scene` filtering was independently verified. [S2, S3]
3. Detail is `GET /api/v1/skillsets/{slug}`. The example has numeric `id: 17`, slug `hr-resume-screening`, displayName `简历筛选`, `scene: hr`, `subScene: resume-screening`. `GET /skillsets/17` returned 404: numeric ID is not interchangeable with the observed slug path. [S4; direct probe]
4. List/detail return `content` and `contentEn`, `skillSlugs`, and `skills: [{slug, namespace}]`, plus summary, icon, publication/scope and timestamps. The example has six members; four have namespaces and two have `namespace: null`. Across all 54 packs, the 324 member occurrences contained 271 populated namespaces and 53 nulls. These are occurrence counts, not unique skill counts. No pack-level publisher namespace or marketplace-version field was present in the observed records. [S3, S4]
5. Shared members exist today: `@clawhub_stevenobiajulu/employment-contract`, `@user_ef64daef/hetongzhushou` and `@clawhub_codefarmerman/legal-doc-writer` each occur in both `legal-contract-drafting` and `legal-contract-generation`. [S3]

## Two different exported ZIP formats

| Producer | Contents and identity evidence |
| --- | --- |
| Backend `/{slug}/download`, consumed by CLI | `manifest.json` + `identify.md`; no member archives. Manifest has `manifestVersion: "1"`, `type: "skillset"`, pack slug/displayName/summary/scene/subScene, ordered `skillSlugs`, descriptive `skills` entries, and workflow file checksum/size. Example manifest member objects omit namespaces and versions. |
| Current website ZIP button | Browser constructs a ZIP with manifest `type: "skillhub-expert-package"`, slug/displayName/summary/skillSlugs and full `skills` coordinates; workflow at `skillsets/{pack-slug}.md`; member downloads at `skills/{member-slug}.zip`. It downloads `@namespace/slug` when the namespace exists, otherwise bare slug. Its manifest does not include pack/member version fields. |

Backend evidence: S5. Website evidence: S2, `SkillPackageDownloadButton`, `getSkillCoordinates`, `getDownloadCoordinate`, `getSkillDownloadUrl`. The website uses API `content` directly when nonempty, preserving workflow frontmatter. Website ZIP creation was inspected in source; its button was not executed.

The website's separate AI-install card points agents to S1. S1 directs `skillhub pack install <slug> --dir <skills-directory>`. Thus the website offers both its browser-built ZIP and the official CLI path; they are observably different artifacts. S1 recommends an Agent's global skill directory, whereas the actual CLI accepts a supplied directory and defaults to `./skills`. [S1, S2, S6]

## Version, pinning and immutability limits

- The live backend ZIP supplied `X-Skillset-Slug: hr-resume-screening` but **no `X-SkillSet-Version`**, no ETag and no Last-Modified header. The CLI reads `X-SkillSet-Version` with default `""`, so this response produces an empty recorded pack version. [S5, S6]
- `manifestVersion: "1"` describes the manifest format. The workflow frontmatter declares `version: 1.0.0`; neither establishes an API-addressable pack release. API list/detail and this ZIP manifest have no pack release-version field. [S3–S5]
- `GET /skillsets/hr-resume-screening/download?version=definitely-does-not-exist` returned 200 with the same manifest and workflow SHA-256 values as the unqualified download. The detail endpoint also ignored that version parameter. `GET /skillsets/hr-resume-screening/versions` returned 405. These probes establish that the tested query does not pin a pack; they do not prove no other version API exists.
- Backend manifest SHA-256: `813a293d4ea3a306faab80994f3e86ef75ef4dbabd19c214a9af0571ebe69d5b`. Workflow SHA-256: `108ea26e92a4d7b28095a7b70124915dd3a52a4d39b5b0bbefc092560f38495c`, matching manifest `files[0].sha256`; workflow size is 4,231 bytes. CLI pack installation does not verify this manifest checksum. [S5, S6]
- CLI parser exposes only `pack install`, with slug, `--force`, `--dir`, `--json`; no pack version or namespace argument. Member URLs use `https://api.skillhub.cn/api/v1/download?slug={bare-slug}` by default, with no requested version. It calls member extraction with `expected_sha256=""`. Website member-download calls also omit a version. [S2, S6]
- A member's independent detail can expose its own `latestVersion`: `@clawhub_heyweekly/easy-recruitment` reports `1.0.0`, also present in that member archive's `_meta.json`. The pack does not pin that member version. No immutable-release guarantee was found or established for either pack or member content. [S4, S5, S7]

## Workflow, member directories and names

- CLI collects members from top-level `skillSlugs` (single pack), or `skillSets[].skillSlugs` (combination), then deduplicates exact slug strings while preserving order. It installs every member directly at `<install-root>/<member-slug>/`; it does not create a containing pack directory around members. It installs `identify.md` as `<install-root>/<requested-pack-slug>/SKILL.md`. [S6]
- Example workflow frontmatter uses `name: hr-resume-screening`, `metadata.package_type: meta-skill`, aliases, and `orchestration.children` containing the six member slugs. Its prose supplies a six-step workflow and invokes members by display/original names. CLI writes that text unchanged. [S4–S6]
- Concrete identity mismatch: API member slug `easy-recruitment`, namespace `clawhub_heyweekly`, displayName `recruitment-assistant`; downloaded member `SKILL.md` declares `name: recruitment-assistant`. Workflow `orchestration.children` says `easy-recruitment`, while its step text says `recruitment-assistant`. CLI installs this archive into an `easy-recruitment` directory and does not rewrite either document. [S4, S6, S7]

## CLI existing files, partial failures and lock semantics

- With no `--force`, any existing member target directory is skipped and appended to the `installed` result without checking identity, version, contents or whether it contains `SKILL.md`. Existing workflow `SKILL.md` is similarly skipped and counted in `installedSkillSets`. `--force` replaces member directories after their individual download/extraction; workflow replacement writes its `SKILL.md`. There is no whole-pack transaction or rollback in this function. [S6]
- Member failures raised as `SystemExit` are caught and collected as `{slug, error: "install failed"}`; subsequent members still run. The workflow prompt and lock are then written even when member failures exist. JSON reports `success: false`, installed/skipped members and failed members, but this failure branch returns normally without a nonzero exit. `main()` simply dispatches `args.func(args)`, so these caught partial failures ordinarily exit 0. Other uncaught exceptions can abort earlier. This is source analysis, not an executed failure test. [S6]
- Lock location is `<install-root>/.skills_store_lock.json`. `packs[packSlug]` records displayName, response-header version, **all declared member slugs**, and installedAt, regardless of member failure or workflow skip. [S6]
- For each successful or skipped member, `skills[slug]` is created only if absent, with `name: slug`, `source: "pack:<first-containing-pack>"`, empty `version`, and installedAt. Existing member lock entries are retained even on force replacement. These entries omit namespace, member checksum, resolved download version and multiple-pack ownership. The workflow is recorded as a pack entry, not independently added to `skills` by this code. [S6]

## SOUL combination boundary

- The same CLI function recognizes an archive containing `skillsets/*.md`, optionally `soul.md`, plus manifest `skillSets[]`. It writes each workflow as `<install-root>/<skillset-slug>/SKILL.md`, deduplicates members across the combination, and writes `SOUL.md` to **the parent of install-root**. Existing SOUL is skipped unless forced. [S6]
- If manifest `soul` metadata exists, CLI updates lock `soul.slug`, displayName and installedAt even if an existing SOUL file was skipped. This branch does not record the SOUL checksum/version. [S6]
- The public single-pack sample contains no SOUL. The website has a separate combination flow calling authenticated `POST /api/v1/activity/20260415/combinations` with `{soulSlug, skillSetSlugs}`. No combination was created and no authenticated combination archive was fetched. The parser's combination support therefore does **not** establish that arbitrary combination slugs can be downloaded through the public pack endpoint. [S2, S5, S6]

## Unresolved facts for the design interview

- Whether SkillHub promises stable pack slugs/numeric IDs, a pack version-history API, immutable pack snapshots, or atomic membership/workflow snapshots is unverified.
- Null member namespace resolution, bare-slug ambiguity policy and slug reassignment guarantees remain unverified. The current API coordinates and CLI pack resolution differ observably.
- The provider has not been shown to promise backward compatibility for these public endpoints or either ZIP schema.
- No runtime agent test was performed to establish how every supported Agent resolves the workflow's member-slug metadata versus each member's original `name`.
- SOUL combination creation/download authorization, archive identity/version and the exact endpoint supplying a combination archive remain unverified.

Repository context: ADR-0030 currently defines explicit member Marketplace coordinates/versions and publication under each original skill name. The upstream pack facts above do not establish a pack-level equivalent. Any product boundary or exception is still a design decision, not a fact inferred from these APIs.

## Follow-up: workflow identity and missing namespaces

Observed **2026-11-11**, completed at 04:25 UTC / 13:25 Asia/Tokyo. Coverage: all 54 Chinese API workflows and all 24 nonempty English workflows in S3; all 47 unique bare slugs behind its 53 null-namespace member occurrences; eight backend workflow exports; original `SKILL.md` names for 36 members across six packs. HTTP reads and text inspection only. This supplements the earlier unresolved questions; no Agent compatibility test was performed.

### Source-format gap: 30 workflows lack skill identity frontmatter

The 54 Chinese workflows have two distinct frontmatter shapes. **24** contain `name`, `description`, `version`, metadata and `orchestration.children`; **30** instead contain only `scene`, `sub_scene` and `skills`, with no `name` or `description`. In all 54, the ordered member list in frontmatter exactly equals API `skillSlugs` (using `orchestration.children` or `skills`, as applicable). Only the 24 workflows with the first shape have nonempty `contentEn` in this snapshot. [S3]

Concrete source example: <https://api.skillhub.cn/api/v1/skillsets/legal-legal-research> and <https://api.skillhub.cn/api/v1/skillsets/legal-legal-research/download>. Both its API content and exported `identify.md` begin with `scene: "legal"`, `sub_scene: "legal-research"`, then `skills: [...]`. The export does not add `name` or `description`. The same omission was checked in exports for `finance-financial-report-analysis`, `academic-gaokao-expert` and `tech-code-refactoring`. This is an observed document-format gap, not a claim about how an Agent will load it.

The 30 affected pack slugs are: `legal-legal-research`, `finance-financial-report-analysis`, `finance-quant-backtesting`, `academic-statistical-analysis`, `finance-business-analysis`, `finance-investment-research`, `marketing-event-planning`, `marketing-user-growth`, `tech-code-refactoring`, `marketing-competitor-analysis`, `academic-paper-search`, `design-ui-prototype`, `ecommerce-product-copywriting`, `ecommerce-promotion-planning`, `education-quiz-generation`, `marketing-social-media-operation`, `media-short-video-copy`, `content-creation-novel-writing`, `academic-gaokao-expert`, `mysticism-bazi-analysis`, `ecommerce-product-selection`, `finance-risk-assessment`, `legal-contract-generation`, `lifestyle-health-data-analysis`, `lifestyle-fitness-training`, `lifestyle-nutrition-meal-plan`, `mysticism-lunar-almanac`, `mysticism-yijing-divination`, `mysticism-ziwei-doushu`, `ecommerce-pricing-analysis`. [S3]

### Null namespaces resolve to explicit coordinates in this snapshot

For **all 47 unique slugs** whose pack `skills[].namespace` is null, unauthenticated `GET /api/v1/skills/{slug}` returned HTTP 200 and one nonempty `namespace` object containing `handle`, `publicSlug` and `canonicalName`. All returned public slugs matched the requested slug. No tested request returned 404 or 409. The deployed website's `fetchSkillDetail` explicitly handles HTTP 409 as an ambiguous public coordinate. [S2; direct reads of each null-namespace slug in S3]

| Bare member slug | Returned complete coordinate | Direct source |
| --- | --- | --- |
| `resume-screening` | `@clawhub_yuyang-rgb094/resume-screening` | <https://api.skillhub.cn/api/v1/skills/resume-screening> |
| `resume-parser` | `@clawhub_ayalili/resume-parser` | <https://api.skillhub.cn/api/v1/skills/resume-parser> |
| `seedance2-storyboard-generator` | `@indiv-liangdabiao/seedance2-storyboard-generator` | <https://api.skillhub.cn/api/v1/skills/seedance2-storyboard-generator> |
| `tencent-docs` | `@tencent-adm/tencent-docs` | <https://api.skillhub.cn/api/v1/skills/tencent-docs> |

This resolves the immediate missing-coordinate fact for the observed catalog. A single successful response establishes the service's current resolution; it does not prove global slug uniqueness, permanence or what a future ambiguous response will contain. No account or enterprise lookup was used.

### Member names and workflow references differ in concrete examples

Read all six members for each of `hr-resume-screening`, `legal-legal-research`, `finance-financial-report-analysis`, `media-storyboard-design`, `tech-code-refactoring` and `design-prd-writing` through the official file endpoint `GET /api/v1/skills/{slug}/file?path=SKILL.md&namespace={handle}`. All 36 returned 200 and a `name` field; **six names differ from the marketplace slug**:

| Pack | Marketplace coordinate | Member `SKILL.md` name | Workflow prose reference |
| --- | --- | --- | --- |
| `hr-resume-screening` | `@clawhub_heyweekly/easy-recruitment` | `recruitment-assistant` | `recruitment-assistant` |
| `legal-legal-research` | `@user_b096d7e1/legalkb` | `legal-kb` | `legalkb` |
| `finance-financial-report-analysis` | `@clawhub_lidayan/tushare-data` | `tushare` | `tushare-data` |
| `media-storyboard-design` | `@clawhub_wild-river2016/seedance2-video-prompter` | `seedance2.0-video-prompter` | `Seedance 2.0 video prompter` |
| `media-storyboard-design` | `@indiv-liangdabiao/seedance2-storyboard-generator` | `seedance-storyboard-generator` | `Seedance2-Storyboard-Generator` |
| `design-prd-writing` | `@clawhub_ryan-wuxl/prd-writer-pro` | `prd-writer` | `PRD文档撰写助手Pro` |

Sources: S3 for workflow content and the official file endpoint above for each coordinate. Strong direct examples: <https://api.skillhub.cn/api/v1/skills/legalkb/file?path=SKILL.md&namespace=user_b096d7e1>, <https://api.skillhub.cn/api/v1/skills/tushare-data/file?path=SKILL.md&namespace=clawhub_lidayan>. In the legal and financial examples, both the workflow frontmatter and its prose use the marketplace slug, whereas the member declares a different original name. This is stronger evidence than a mere difference between display casing and slug punctuation, but it still does not establish runtime failure.

A lexical scan of the **54 Chinese + 24 English workflow texts** found zero fenced code blocks, zero `SKILL.md` references, zero `skills/`, `references/`, `scripts/`, `.agents/`, `.claude/` or `.codex/` paths, and zero command-shaped uses of `skillhub`, `clawhub`, `npx`, `uvx`, `pip`, `python`, `node` or `bash`. Thus this corpus supplied no literal filesystem or installation-command reference to a member-slug directory. Natural-language member references and frontmatter references remain present. This scan covers the pack workflows, not every member's own skill content. [S3]

### API content and backend workflow agree except for trailing newline

For each of these eight packs, the exported `identify.md` was **exactly API `content` plus one LF newline**, with no other change: `hr-resume-screening`, `tech-test-automation`, `legal-legal-research`, `finance-financial-report-analysis`, `academic-gaokao-expert`, `media-storyboard-design`, `tech-code-refactoring`, `design-prd-writing`. Source pair for every row: `GET /api/v1/skillsets/{slug}` / S3 content and `GET /api/v1/skillsets/{slug}/download`. Raw byte hashes consequently differ; no normalization of missing frontmatter or member names was observed.

Example hashes: `legal-legal-research` API content SHA-256 `7e7f16bd913dc8f7574f0e680370d885a32aaf46fb22c4e669499b43fd6ab630`; exported workflow `e85589db0686e302ad7cbd40895bd68b06f746e8eeec5c786f80fc7242dfbbf7`. HR API content SHA-256 `4844dd14d668d4f8b2af69d76f3632f7bf8351fcace91e6eb60519c68e956c9e`; exported workflow remains `108ea26e92a4d7b28095a7b70124915dd3a52a4d39b5b0bbefc092560f38495c`.

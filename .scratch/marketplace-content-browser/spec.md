# Marketplace Content browser: concentrate browsing behavior

Date: 2026-11-21.
Status: implemented and verified on 2026-11-21.

## Problem and intended result

Architecture candidate 3 identified a shallow shared file-tree module. SkillHub,
ClawHub and skills.sh callers still coordinate selection, expanded directories,
tabs, raw mode, relative links, focus and File preview. Sharing tree drawing
alone leaves the same browsing protocol in three places.

Deepen the Marketplace Content browser module so those behaviors sit behind a
small interface. Three real source adapters preserve versioned reads and whole
Marketplace snapshot reads at an internal seam. The browser provides leverage
to source pages and locality for navigation and preview changes.

## Scope

The user requested the next candidate after candidate 2 was committed as
`5cdef74`. Continue the behavior-preserving scope of the earlier candidates:
preserve visible behavior, source identity, request timing, refresh and error
policy. Independently discovered defects belong in separate issues.

Concentrate browsing for all three existing Marketplace sources. Do not merge
their entire pages, include the installed local Skill Content browser, change
installation behavior, or add a backend/IPC contract or dependency. Existing
Markdown sanitization and relative-path helpers remain authoritative.

## Ownership

| Owner | Responsibilities |
| --- | --- |
| Content browser | Tab activation and keyboard navigation; selected file and expanded ancestors; tree construction/rendering; raw/rendered reading state; relative-file navigation; focus and scrolling; shared File preview layout and rendering; browsing-session reset |
| SkillHub adapter | Detail Markdown as Overview; lazy versioned listing and file queries; missing-path/read/error state and retry behavior; source-specific reading lifecycle and presentation facts |
| ClawHub adapter | Selected-version listing and SKILL.md queries; per-file queries; derived default path; license/changelog provenance; version refresh; same-version retained-data notices; canonical links and source-specific error states |
| skills.sh adapter | Whole-snapshot query and refresh; snapshot notices and unavailable-content states; in-memory file lookup; successful-response browsing reset; source-specific reading lifecycle and presentation facts |
| Source pages | Catalog/search; detail header and source classification; ClawHub version selection; Scope; installation eligibility, confirmation and actions; source switching and return navigation |

The three adapters are private to the Content browser module. Their source
facts inform one browsing implementation; they do not each retain a copy of
selection, directory expansion, tab keyboard handlers or navigation handlers.
Source-specific messages and metadata may retain private rendering helpers.

The local installed Skill Content browser keeps Finder, disk refresh and
open-file responsibilities in `skill-content-tab.tsx` and is not migrated.

## Public interface and internal seam

Use one Content browser entry with a closed source descriptor and activity:

```tsx
type MarketplaceContentSource =
  | { kind: "skillhub"; detail: MarketplaceDetail }
  | {
      kind: "clawhub";
      coordinate: ClawHubCoordinate;
      version: string;
      canonicalUrl: string;
      changelog: string | null;
    }
  | { kind: "skills-sh"; skill: SkillsShSkill };

<MarketplaceContentBrowser source={source} active={active} />
```

These are existing identity/content facts, not browsing state. The ClawHub
changelog is the selected version's detail-list fallback. The skills.sh skill
supplies its coordinate and existing metadata needed for unavailable-snapshot
messages; its Git source is not a browsing version.

Concrete names and private file layout are implementation choices. Callers do
not pass tab, raw mode, selected path, expanded directories, refs, navigation
callbacks, query state or a sequence of loading operations. They do not need to
key the browser to reset it: the browser owns identity/version/session resets.
Recreating a descriptor object during a Scope or installation render must not
reset the browser.

At the internal seam, adapters retain loading, query identity and error/retained
data policy. The common browsing implementation consumes files, the applicable
default path, Overview/File preview content and source-owned read states. It
supplies its current tab/path to the adapter when those facts determine a read.
Versioned File preview uses the existing `SkillFileContent`; the snapshot
adapter keeps `SkillsShFile.contents: null` distinguishable rather than
manufacturing image bytes or a versioned response.

Use closed dispatch for the three actual adapters. Do not add an extensible
registry, public strategy/configuration framework or generic resource loader.
Source-specific reset/focus policy stays private and does not expand the public
interface. Existing backend injection and Fake backend are sufficient for
testing; no new dependency-injection surface is needed.

## Source contracts that must survive

### Loading, cache and refresh

| Concern | SkillHub | ClawHub | skills.sh |
| --- | --- | --- | --- |
| Overview content | Markdown from the detail response | Exact selected version's SKILL.md | SKILL.md in the captured snapshot |
| Listing timing | First Files activation/link navigation; enabled only while Files is active | Listing and SKILL.md start in parallel for an active selected version | Included in the one snapshot response |
| File reads | Per-file IPC for a listed path while its preview is mounted | Per-file IPC for a listed path while Files is active; Overview shares the SKILL.md query identity | In-memory lookup; no per-file IPC |
| Freshness/retention | Infinity stale time, five-minute inactive GC | Infinity stale time, five-minute inactive GC | Sixty-second stale time and existing default retention |
| Automatic retry | Disabled | Disabled | Disabled |
| Refresh | Existing detail/list/file retries | Refetch listing and SKILL.md when present; other selected files remain cached | Refetch and replace the complete snapshot after success |
| Read/refresh failure | Listing/file error takes display precedence over their prior data | Preserve successful same-version data with a stale notice | Preserve the prior complete snapshot and browsing state |

Preserve the current enabled conditions rather than introducing a global gate:
SkillHub browsing follows the Files tab, independent of the page's `active`
prop; ClawHub source activity gates listing and Overview, while its existing
File preview read is gated by Files and path existence. skills.sh activity gates
the snapshot query. Moving a query must not eagerly load SkillHub files or
serialize ClawHub's initially parallel requests.

Query keys remain exactly:

- SkillHub list: `["marketplace", "files", marketplaceKey(coordinate), version]`.
- SkillHub file: `["marketplace", "file", marketplaceKey(coordinate), version, path]`.
- ClawHub list: `["marketplace", "clawhub", "files", clawhubKey(coordinate), version]`.
- ClawHub file/Overview: `["marketplace", "clawhub", "file", clawhubKey(coordinate), version, path]`.
- skills.sh snapshot: `["marketplace", "skills-sh", "snapshot", skillsShKey(coordinate)]`.

Changing coordinate/source/version isolates content from late responses and
resets browsing. Same-version SkillHub/ClawHub updates preserve selection and
expansion. A successful skills.sh response resets all browsing using
`dataUpdatedAt`, even with an unchanged snapshot hash; a failed refresh retains
the complete prior snapshot and session. Do not use snapshot hash as a
Marketplace version or replace its files incrementally.

### Selection and raw mode

| Concern | SkillHub | ClawHub | skills.sh |
| --- | --- | --- | --- |
| Default file | SKILL.md, including when absent | SKILL.md if listed, otherwise the first listed file; derive this while no explicit path is selected | SKILL.md, including when absent |
| Overview raw mode | Survives tab switches | Resets on tab activation/navigation | One raw state, reset on tab activation/navigation |
| Files raw mode | Separate state; resets on file-path change and after leaving Files | Separate state; persists across file-path changes while preview stays mounted; resets when leaving Files | Same raw state as Overview |
| Scope/install changes | Preserve file, expansion and reading state according to current mounting | Same | Same |

Do not reset ClawHub's explicit path when a refreshed listing removes it. Show
the missing-path state. If a successful listing omits SKILL.md, ignore the
Overview query result, including an earlier successful response. Default-path
derivation must not depend on an effect that temporarily displays another file.

### Navigation, rendering and accessibility

- All sources retain ArrowLeft/ArrowRight/Home/End tab activation, roving tab
  focus, existing tab/panel IDs and accessible labels. Preserve hidden panels
  and source-specific mount lifetimes where they determine reading state.
- Selecting a file expands its ancestors. Selecting a directory only toggles
  that directory. Relative-file links stay within the same Content browser.
- SkillHub Overview links focus the Files tab and scroll the tab section;
  file navigation focuses and scrolls the preview. ClawHub currently attempts
  to focus the preview synchronously; Overview navigation may precede mounting
  that ref. skills.sh keeps its preview mounted across tab changes, allowing
  immediate preview focus. Preserve these differences in this refactor.
- Reuse `SkillMarkdown` and `skill-content` path helpers. Keep sanitized
  Markdown, controlled external links and restricted remote images. Raw text
  remains text; browsing remains read-only.
- Versioned sources keep Markdown, text, image, too-large, binary and unreadable
  states, sizes and limits. Preserve source-specific messages. skills.sh
  `contents: null` retains its unsupported/oversized-text explanation and never
  becomes an image/base64 preview.
- Missing file, empty version, unavailable version and unavailable snapshot are
  distinct states. `snapshot.files: null` does not mean the upstream has no
  files. Keep source-specific retry buttons, loading indicators, official links
  and retained-data notices.
- ClawHub license comes only from the selected-version listing response.
  Its changelog prefers that response and falls back to the same selected
  version's detail-list entry. Latest metadata must not leak into another
  selected version. Alias and no-version/GitHub-backed handling stay in the
  ClawHub source page.
- Preserve existing desktop/narrow-window layout and scroll behavior; this is
  an ownership change, not a visual redesign.

The possible existing ClawHub Overview link-base defect is recorded in
[issue 01](issues/01-clawhub-overview-link-base.md). It is a static inference,
not a reproduced finding, and is not silently fixed or promoted to an intended
product contract by this refactor.

## Migration and architecture acceptance

Move the browsing responsibilities currently in `marketplace-detail-tabs.tsx`,
`marketplace-files-tab.tsx`, `ClawHubVersionBody`/its preview helpers, and
`SnapshotContent` behind the new interface. Move skills.sh's snapshot query and
its content notices with the snapshot adapter. Keep source page headers,
classification, version selection and installation concerns in their callers.

After all three callers migrate, remove obsolete public tree-only and
tab/file-wrapper interfaces. Private tree/preview helpers may remain where they
concentrate real work; do not retain forwarding wrappers solely to avoid
updating imports or tests. Avoid adjacent cleanup.

Acceptance is observable in callers: none owns a tab/path/expansion/raw state
protocol or assembles preview navigation callbacks. One browser implementation
owns shared browsing behavior; adapters contain actual reading differences.
The deletion test is that removing this browser would scatter navigation and
preview coordination across all three source pages again. A mere relocation
of three complete browsers behind a dispatcher does not meet the candidate.

No new domain term is needed. ADRs 0029, 0033 and 0034 stay in force:
Marketplace identity remains separate from discovery/installation identity;
snapshot content does not pin a Git installation; ClawHub registry identity
does not merge with Git identity. Installation, native validation, transport
cache and rate limiting remain unchanged.

## Implementation and verification plan

1. Establish the baseline and record existing source contracts. Completed on
   2026-11-21: the five relevant files below passed all 66 tests; log
   `/tmp/marketplace-content-baseline.log`.
2. Implement the shared browsing module and private SkillHub/ClawHub adapters,
   retaining source query timing and read states. Migrate their callers and
   verify the focused tests before migrating the whole-snapshot adapter.
3. Migrate skills.sh query/notices/session replacement and remove obsolete
   browsing interfaces. Verify snapshot refresh and installation independence.
4. Keep existing App/Fake backend tests as the test surface. Add small focused
   cases for source raw-mode lifetimes, ClawHub/skills.sh keyboard tabs and
   delayed responses after changing the selected file where current coverage
   is missing. Assert rendered content, focus and requests, not private state
   or extracted tree helpers. Retain version/source race and stale-content
   coverage; replace obsolete helper tests only after equivalent coverage.
5. Run the focused suite, then the full frontend suite, `pnpm build` and
   `git diff --check`. Inspect all migrated callers for leaked browsing state
   and duplicate handlers. Use the existing Fake backend in a real browser
   for desktop/minimum-window layout, keyboard focus and relative-file
   navigation. Native/network tests are unnecessary unless implementation
   changes extend beyond this frontend-only scope.

Focused baseline command:

```sh
pnpm test src/App.marketplace-files.test.tsx \
  src/App.clawhub-detail.test.tsx src/App.skills-sh.test.tsx \
  src/App.marketplace.test.tsx src/lib/skill-content.test.ts
```

## Final confirmation

The user confirmed the complete ownership split, behavior-preserving
constraints and verification plan on 2026-11-21.

## Implementation results

- `src/components/marketplace-content-browser/index.tsx` is the single public
  interface. It owns coordinate/version resets; the snapshot adapter owns
  successful-response resets using `dataUpdatedAt`.
- `browser.tsx` owns browsing state, directory expansion, relative-file
  navigation, keyboard tabs, panels, focus/scrolling and tree/preview layout.
  `preview.tsx` shares versioned-file rendering and private file raw state.
  Three private source adapters retain their actual query and content policies.
- All three source pages now supply source facts and activity. They no longer
  own browsing state or preview callbacks. The obsolete
  `marketplace-detail-tabs.tsx` and `marketplace-files-tab.tsx` were removed.
  Existing installation, metadata and ClawHub version-selection code remains
  with its source page.
- Five tests at the public Content browser interface cover the three distinct
  raw-mode lifetimes, ClawHub/skills.sh keyboard navigation, descriptor rerenders,
  unchanged-snapshot successful refresh and both versioned sources' delayed
  file responses. Existing App tests remain unchanged.

Validation on 2026-11-21:

| Check | Result |
| --- | --- |
| SkillHub/ClawHub migration regression | 3 files, 41 tests passed |
| All existing focused tests after snapshot migration | 5 files, 66 tests passed |
| Final focused suite including browser-interface tests | 6 files, 71 tests passed |
| Full `pnpm test` | 52 files, 536 tests passed; log `/tmp/marketplace-content-full-tests.log` |
| `pnpm build` | TypeScript and production build passed; existing main-bundle size warning remains |
| Changed/new-file whitespace | `git diff --check` and explicit new-file checks passed |
| Real browser with existing App/Fake backend | Three sources at 1440, 1100 (native minimum) and 390px; no document/content/preview horizontal overflow; desktop side-by-side and narrow stacked layout retained |
| Real browser interactions | Relative-file navigation, ancestor expansion, preview focus, keyboard activation/focus and selected-path retention passed |

Browser measurements are recorded in
[browser-checks.json](browser-checks.json). Temporary browser screenshots were
visually inspected; no screenshots or preview-only application code were added
to the repository. The task's isolated browser and Vite server were closed.

No native/network tests or installation execution were rerun because this
change is confined to frontend ownership. The separately recorded ClawHub
Overview link-base inference remains outside this refactor. The user requested
committing the verified implementation on 2026-11-21.

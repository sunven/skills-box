# ClawHub Overview relative links may use the selected file's directory

Status: needs-triage
Date: 2026-11-21.
Evidence: static code inference; not reproduced at runtime.

## Finding

`ClawHubVersionBody` constructs Overview's `fileLinks` with
`baseDir: parentDir(selectedPath)` in
`src/components/clawhub-detail-page.tsx` near line 378 and passes it to the
Overview `SkillFileBody` near line 495. Returning to Overview preserves the
explicit selected file path.

After selecting `references/guide.md`, then returning to Overview, a SKILL.md
link to `references/guide.md` may therefore resolve as
`references/references/guide.md`. Overview is still displaying root SKILL.md,
so its relative links should use that file's directory rather than the last
selected file's directory. The Files preview separately derives its base from
its own displayed path.

## Proposed reproduction and acceptance

Use the existing App/Fake backend with a root SKILL.md containing a
`references/guide.md` link and that nested file in the selected version.
Navigate to the nested file, return to Overview, then follow the root link
again. Confirm the selected/read path before classifying this as a reproduced
defect. Existing tests do not exercise this sequence.

If confirmed and accepted for a separate fix, Overview links should resolve
against root SKILL.md; Files links should continue to resolve against the
displayed file. Keep the selected version, selection retention and preview
request policy intact.

## Scope

Found during architecture candidate 3. The behavior-preserving Content browser
refactor does not include this fix. No runtime reproduction or application code
change has been made for this issue.

## Comments

2026-11-21: Read-only fact-finding independently confirmed the static inference
and the missing sequence in current test coverage.

2026-11-21: Candidate 3 moved the unchanged Overview base calculation to
`src/components/marketplace-content-browser/clawhub.tsx` near line 40. The
source adapter still uses the selected path's parent directory. This issue
remains a static inference; no fix or dedicated reproduction was included.

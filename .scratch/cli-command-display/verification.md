# CLI command display verification

Verified on 2026-11-05.

## Behavior

- Source preview, Install, Update/Update all, Uninstall and Batch operations show commands before execution. Retry previews use only the selected failed or unexecuted targets.
- Rust formats the validated CommandSpec as `cd <actual cwd> && <absolute npx path> -y skills@latest …`, including shell quoting. The frontend displays the returned text.
- Mutation starts validate the preview before task registration and compare the actual command before invocation. Changed sources, Agents or runtime paths require a refreshed preview and another click.
- Running and result views retain actual invocations and their CLI outcome. Environment diagnostics show the latest version probe; Discovery source records show the latest catalog invocation and its start time.
- Native Uninstall, skipped Update targets, cache hits and cancellation before invocation produce no actual CLI record. Records remain in memory for the current app session.

## Final automated checks

- `pnpm test`: 320 tests passed across 37 files.
- `cargo test` in `src-tauri`: 653 tests passed, including 620 library tests and 33 integration tests. Four network-dependent tests remain explicitly ignored.
- `pnpm build`: passed. The existing Vite chunk-size warning remains.
- `cargo fmt --check` and `git diff --check`: passed.

Regression coverage includes preview/actual-command agreement, shell quoting, stale-preview rejection before registration or spawn, runtime changes after registration, native and skipped-only operations without a CLI runtime, batch command validation, invocation outcomes and cancellation, snapshot/event merging, cache hits and retry target preservation.

## Browser checks

Used the fake backend in the named agent-browser session `cli-commands-56f962d97da4`; no installed user Skills were changed.

- Checked source preview, Install confirmation, Update and result, single Uninstall and result, Batch Uninstall and result, Environment diagnostics and Discovery calls.
- Reviewed the built preview at desktop (1440px), tablet (768px) and mobile (390px) widths. The mobile page had no horizontal overflow.
- Confirmed a repeated cached Discovery scan retains the earlier actual command rather than inventing a new invocation.
- Changed Environment command records to a full-width, collapsed section after screenshot inspection; expanded records have bounded scrolling.
- The new command display passed the taste linter with zero failures or warnings. The production browser inspector also reported zero findings.

The inspector used two explicit allowances: `placeholder-data` because shared fake preview fixtures contain Acme sources, and `covered-text` because an open confirmation dialog intentionally covers the background page heading. Neither allowance changes production behavior.

Production inspector evidence: [report](production-inspect/report.json), [desktop](production-inspect/desktop-fold.jpg), [tablet](production-inspect/tablet-fold.jpg), [mobile](production-inspect/mobile-fold.jpg).
Additional flow screenshots are in [flows](flows/).

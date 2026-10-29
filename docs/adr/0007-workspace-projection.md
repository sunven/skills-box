# The workspace projection owns derived UI state

Status: accepted

`App` built the workspace from twelve `useMemo`s across the inventory snapshot, update report, filters, selection, counts, anomalies, update summary and dialog targets. `UpdateDialog` recomputed `computeUpdateSummary` and its own single/all eligibility, and `DetailPane` merged the selected row with the async detail. We introduce a pure `buildWorkspaceView` in `src/lib/workspace-view.ts` that owns every derived value: overlay rows, visible rows, selected skill, status counts, source/agent options, anomalies, update summary, `UpdateDialogView`, totals and last-check state. App keeps filters, selection, dialog flags, `useDeferredValue`, the selection fallback effect and the uninstall target snapshot, and calls the projection in one `useMemo`. `selectUpdateDialog` in `update-view.ts` owns the dialog summary/grouping/initial names/single-vs-all eligibility; `selectDetailView` in `detail-view.ts` owns the row/detail merge and alert derivations. Components keep their narrow prop interfaces.

## Considered options

- **A workspace controller/store**: rejected for this change because it would mix UI state ownership with the derived projection and make pure tests harder; App keeps state and orchestration.
- **Passing the whole WorkspaceView to every surface**: rejected because it would replace explicit narrow props with a wide shared object; the projection's value is derived-state locality, not prop consolidation.
- **Moving the detail query into App/projection**: rejected because the query lifecycle, tabs and detach/attach state are local to DetailPane; only the pure merge moved.
- **Folding all selectors into one file**: rejected; workspace composition, update-dialog rules and detail merge are separate domains with separate tests.

## Consequences

- App has one workspace `useMemo`; adding a derived field no longer means another App memo or a second component calculation.
- UpdateDialog keeps only `confirmed`, task-runner state, retry names and open/close; the single-vs-all eligibility divergence is explicit in `selectUpdateDialog`.
- DetailPane keeps its query, tabs and action state; `selectDetailView` owns the row/detail merge and warning/alert derivations.
- Behaviour is preserved: deferred filtering, selection fallback, update overlay, single-vs-all eligibility, detail warnings replacing row warnings when detail exists, dialog confirm/retry and the uninstall snapshot.
- The projection still returns to App as a plain value; no context, store, backend or IPC change is introduced.

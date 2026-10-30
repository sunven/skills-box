# Project Update checks publish against a persisted revision

Status: accepted

A Check attempt can finish after Relocate or Forget, including Relocate to the
same Project root or A -> B -> A. Root equality cannot establish validity, and
best-effort deletion alone cannot hide residual summaries after a restart. The
Project registry therefore owns an opaque persisted check revision, changed by
every successful Relocate. Project summaries are valid only for the registered
Project's current revision; legacy Project summaries without a revision are
discarded, while Global history is preserved.

One Project Update check module owns capture, publication, registry-change
invalidation and valid-summary reads. Its coordination lock protects only those
short operations; the network remains outside both this lock and the mutation
permit, preserving ADR 0013. An Invalidated check ends normally without a report
or summary. A successful registry change survives cleanup failure and carries a
typed warning; the persisted revision or missing registry entry hides old state
even when cleanup failed.

The frontend captures Scope and revision at submission and shares pending state
by that identity. Different Scopes and new revisions run independently; an old
completion cannot publish into another Scope or clear a new pending check.
Relocate clears live report, badge and Last check values in place, then the new
revision receives an immediate full check after its available Project root's
Managed skill inventory refreshes. Active-Scope switching preserves validity.

Behavior is exercised through the lifecycle interface in
`src-tauri/tests/project_check_lifecycle.rs`, the shared hooks in
`src/hooks/use-updates.test.tsx`, and the app flows in
`src/App.project-check-lifecycle.test.tsx`.

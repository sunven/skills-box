# Native plugin operations share coordination with separate recovery semantics

Status: accepted

Plugin and Plugin marketplace writes join the application's existing mutation permit, active-operation tracking, shutdown and App update coordination. The worker retains ownership through native execution and state verification. Plugin operations use their own identities, results and interruption records rather than entering the Skill snapshot, lock and Canonical store protocol from ADR-0003.

Native tools own Plugin installation, configuration and related side effects, so a Skill backup cannot establish a complete rollback point. Skills Box accepts partially completed operations, reports the observed native state and requires revalidation before retrying; the first release provides no complete Plugin backup or automatic rollback. Keeping a second installation ledger or reconstructing native state from Skill snapshots would violate ADR-0037 and could overwrite changes made by the host tools.

Once Plugin writing begins, normal cancellation is sealed and normal application exit waits for the current operation and verification to finish. Skills Box does not offer forced interruption for that write; an abnormal process or system interruption leaves an operation requiring state reconciliation. This deliberately differs from the existing Skill single-task confirmed-quit behavior, while retaining shared exclusion with App update installation. The recovery and exit choices were confirmed as Q18 and Q19 in the 2026-11-19 [design interview](../../.scratch/native-plugin-management/spec.md); integration evidence is recorded in the [lifecycle investigation](../../.scratch/native-plugin-management/research/03-operation-lifecycle-integration.md).

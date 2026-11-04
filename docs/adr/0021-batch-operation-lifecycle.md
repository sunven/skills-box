# Batch operations own one registered task and one Scope

Status: accepted

Batch operations apply Update, Uninstall, Attach or Detach to a confirmed set of Batch targets in one Scope. Each batch is one registered backend Mutation task that owns the single-writer permit and resolved Scope through completion, including batches whose units use native Attach or Detach. Targets execute serially in the confirmed order. Frontend loops over independent single-item IPC calls were rejected because they release ownership between calls, allow other writers or Project root changes to interleave, and leave native batches outside the application's task and exit lifecycle. These boundaries were accepted during the design interview and implemented on 2026-11-04.

The batch owns fixed target identities, per-target outcomes and stopping between targets. Existing action rules still own eligibility, dependency checks, writes, verification and their distinct backup strategies. Confirmed content and attachment scope are checked again before each write; changed or previously excluded targets do not gain new authorization. Ordinary target failures continue, while backup failure or loss of the ability to continue the batch stops later targets without discarding completed results. Successful writes are retained rather than rolled back as one transaction.

Stopping, including an explicit request to stop later targets and exit the application, waits for the current target to settle and marks remaining eligible targets unexecuted. Unexpected termination retains the existing interruption safeguards and does not resume the batch automatically. Results remain in the open result view; retry requires a fresh preview and confirmation for failed or unexecuted targets and never replays successful targets.

This extends ADR-0016's worker ownership to the entire batch and preserves ADR-0013's protection against Project Relocate / Forget. ADR-0003's exclusion of standalone native Attach / Detach from MutationSession remains valid: sharing the outer batch task lifecycle does not require native actions to adopt the CLI action protocol. Standalone operations retain their existing lifecycle.

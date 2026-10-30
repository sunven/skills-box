# Project root physical path facts have one implementation

Status: accepted

Attachment targets, Scope Install targets and Project catalog scanning each
implemented physical containment. Their policies differ, but the duplicated
resolution had drifted: an Agent directory or Canonical store whose parent was
a symlink outside the Project root could still be scanned when the final
directory was ordinary. Separately, the Read model rejected that Skill's
description but still read its files for a Baseline fingerprint.

The `project_path` module concentrates lexical normalization, physical
containment and nearest-existing-ancestor traversal behind one interface.
It returns directory facts or distinct normalization, missing, inspection,
resolution and escape failures. Canonicalizing the whole existing path checks
parent symlinks as well as the final entry. Only NotFound permits climbing to
an ancestor; broken links and other inspection errors are failures. This gives
callers leverage and keeps path-resolution changes local to one implementation.

Callers retain the policies established by ADRs 0005 and 0011. Attachment
targets may be missing and retain the historical physical-root fallback;
Install target selection requires the root to resolve and silently excludes
failed targets. Scanning requires an existing directory and retains its warning
and anomaly presentation. Attachment targets physically resolve the normalized
path; layout/catalog paths preserve the original filesystem semantics. Root
resolution, attachment eligibility, error priority and Install linking's relaxed
profile remain caller decisions. A single boolean or a shared AppError would
erase distinctions those callers need. The filesystem has one concrete
implementation here, so no new adapter or filesystem trait is introduced.

Whole out-of-root directories are excluded from scanning, including direct
catalog fixtures that inject Agent directories. Individual escaping Skill
entries remain Anomaly rows. The Read model validates their primary path once
before description, baseline assessment or directory metadata reads: invalid
Project paths report unknown Local modification and cannot create, update or
erase a baseline. Valid paths still initialize and assess baselines as described
in ADR 0004. Content commands retain their independent strict allowed-root
checks. This does not change Mutation session, permit, IPC or Scope isolation.

Tests exercise the production attachment, ScopeLayout, catalog and Read model
interfaces with real temporary Project roots. The same path cases cover missing
directories, internal/external parent symlinks, final directory symlinks, broken
links and non-directory parents. Baseline cases verify both missing and seeded
records, Managed and Copy-only Skill anomalies, valid-path initialization and
independent Global/other-Project records.

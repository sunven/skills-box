# CLI execution scope comes from the action's layout

Status: accepted

InstallPlan and UpdateItem copied `scope`, `cwd` and `global` from ScopeLayout,
while actions kept another layout for Backup snapshots and verification.
Uninstall and Preview had different interfaces for the same cwd/`-g` pairing.
`ScopedCli` now binds the runtime to the resolved layout and owns command
construction for Install, Update, Uninstall and Preview. Action plans retain
operation data; commands, verification and Baseline fingerprint bookkeeping
use the action's layout. Keeping copied execution fields or returning a
cwd/global pair was rejected because callers would still own the synchronization
rule. The old command helpers are removed rather than wrapped.

This preserves ADR 0011's Scope rules and exact pinned CLI argument ordering.
Global mutations use `runtime.cwd`; Global Preview uses the app's isolated cwd.
The production runtime supplies that same directory, but preserving both
sources keeps adapter behavior unchanged. Project commands use the resolved
Project root without `-g`.

Only command construction is shared: Preview and native actions retain their
lifecycles, and ADRs 0003/0016 retain Mutation session and TaskManager ownership.
The existing ProcessExecutor seam has production and fake adapters; no new
executor interface is needed. Tests observe commands through ScopedCli and
TaskManager, together with temporary Canonical store, Lock entry, Backup
snapshot and baseline state. The independent real-CLI sandbox continues to
validate upstream semantics.

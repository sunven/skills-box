# Unmanaged skills uninstall by their located paths

Status: accepted

[ADR-0023](0023-uninstall-dependency-boundary.md) narrows the copy dependency check to references outside one Skill's confirmed Uninstall scope and adds detection of Codex's existing compatibility directory.

On 2026-11-04 the user explicitly requested Uninstall for Unmanaged skills, including single-item and Batch operations. A missing Lock entry no longer rejects Uninstall. It still rejects Update, Attach and Detach. This revises the original eligibility restriction and adds an exception to ADR-0001's CLI mutation boundary.

Managed and Copy-only skills retain the pinned CLI executor. Unmanaged skills use native deletion of the exact canonical and Agent attachment paths from the Scope inventory: the CLI's directory scan misses canonical symlinks and Extra-agent-only skills, and its name normalization can address a different directory. Native deletion unlinks symlinks without deleting their targets, removes real directories, and leaves the Scope lock untouched. Existing path and copy dependency checks remain in force.

The native writer shares ADR-0003's Mutation session protocol: backup, snapshot registration, cancellation seal, post-backup authorization check, write, verification, Baseline clearing and logging. CLI runtime resolution is skipped for this plan. The shared session gains a native execution hook; standalone Attach and Detach remain outside it. ADR-0021 continues to own batch serialization, confirmed targets, stopping and results.

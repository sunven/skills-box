# Agent attachment eligibility has one gate

Status: accepted

Attach, detach, install's extra-agent linking and the inventory `detachable` flag each re-decoded the management × agent × attachment-mode matrix and the target path guards. The copies had already drifted: install's extra link used a weaker path profile and `AGENT_ATTACH_FAILED` where detach used `PATH_INVALID`, while the UI flag ignored shared agents and physical checks. We introduce a top-level `attachment_target` module with three entry points — `resolve_attach`, `resolve_detach`, `resolve_install_link` — sharing one private target-path/eligibility core. They return a `ResolvedAttachment` (agent, canonical, target, existing attachment, target state, directory state) or a canonical `AppError`. Mutations keep their own writes, backups, dependency scans, verification and logging. `detach_eligibility` is the typed row rule used by detach and the read model; the read model now passes `universal || shared` and maps `Detachable` to the UI flag. Uninstall stays a skill-level plan: it intentionally allows copy-only skills, plans multiple attachments and scans copy dependencies, so it does not use the agent-target gate.

## Considered options

- **One `resolve(intent)` function**: rejected because install has no located skill/attachments and uses a relaxed path profile; forcing it through one shape adds adapters and intent branches without reducing the mutation contracts.
- **Boolean eligibility only**: rejected because the UI and detach would still maintain separate rules; a typed rejection reason lets detach preserve its error codes while the UI only asks `Detachable`.
- **Uninstall through the gate**: rejected because uninstall is skill-level, allows copy-only and does not resolve one agent target.
- **Normalize install's path guards and error codes**: rejected for this refactor; behavior preservation keeps existing attach/detach/install tests as locks.

## Consequences

- `same_dir`, direct-child, canonical-target and agent-directory checks live in one module; attach/detach/install no longer carry copies.
- Install keeps its historical relaxed profile: no direct-child/canonical-target checks, idempotent correct symlinks, and `AGENT_ATTACH_FAILED` for agent-directory problems.
- `detach_eligibility` may correct the UI flag for a shared agent with a link/copy attachment so it no longer shows as detachable.
- Attach and detach still validate the skill id/name, read the lock and locate the skill before calling the gate; the gate keeps their historical error order.
- Detach still performs the physical mode recheck, copy dependency scan, backup, unlink/remove and failure logging; `target_state = Absent` is left for detach to report as `AGENT_NOT_ATTACHED`.
- Uninstall's copy-only allowance and dependency scan remain separate, documented here so future reviews do not try to fold them into the gate.

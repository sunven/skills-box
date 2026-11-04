# Uninstall protects references outside one Skill

Status: accepted

On 2026-11-04 the user requested Uninstall for Skills marked 「源路径缺失」. That update status already permits Uninstall; the actual refusal came from treating a Skill's own Agent links to its copy as outside dependencies. Uninstall now permits dependent links whose exact paths are already in that Skill's canonical and attachment plan, while references outside the plan still reject deletion. This revises ADR-0022's conservative copy dependency rule. Detach still checks all dependents because it removes only one Agent attachment. Selecting another dependent Skill in a Batch operation does not enlarge this per-Skill authorization or reorder targets.

Every Uninstall plan, including CLI plans, rechecks its Scope, attachment scope and copy dependencies after backup and before writing. The existing backup and post-removal verification remain required. Removing a link never follows its target for deletion; a copy already listed as a separate deletion target is removed by its own planned action.

The same installed Skills also have links in `~/.codex/skills`, which the pinned CLI no longer catalogs. That existing global directory is represented as the builtin Extra agent `codex-legacy`, labeled 「Codex（旧目录）」, so its paths appear in inventory and deletion previews and use the existing native Extra-agent cleanup. This extends ADR-0002's overlay and uses directory existence for compatibility detection; the entry is hidden when the skills directory is absent. It has no Project directory mapping. The canonical Codex catalog entry remains the pinned CLI's shared directory.

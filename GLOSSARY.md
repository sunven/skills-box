# Skills Box

A macOS-first desktop tool for managing Agent Skills at global and project scope across coding agents: inventory, install, update, detach, and uninstall.

## Language

**Global skill**:
A skill installed at user scope, available across projects and mounted into one or more agents.
_Avoid_: user skill, system skill

**Project**:
A directory registered in the app whose skills are managed independently of the global scope.
_Avoid_: workspace, folder

**Project root**:
The directory that anchors one project's skills and agent attachments; relocating a project changes its root but not its identity.
_Avoid_: project path, folder

**Relocate**:
Point an existing project at a new root while keeping its identity, used when the old root is unavailable.
_Avoid_: move, rename

**Forget**:
Remove a project from the registry without deleting any skill files or attachments.
_Avoid_: remove project, delete project

**Canonical store**:
The directory that holds one scope's actual skill folders, shared by all agents in that scope.
_Avoid_: cache, source directory

**Skill**:
A folder identified by its `SKILL.md` and name, containing reusable instructions for coding agents.
_Avoid_: package, plugin

**Skill content**:
The files and directories inside a skill folder other than its `SKILL.md` entry point, such as references, scripts, and assets.
_Avoid_: resources, bundled files

**Content browser**:
The read-only tree-and-preview view of one skill's content.
_Avoid_: file explorer, file manager

**File preview**:
The read-only rendering of one skill content file: rendered Markdown, raw text, an image, or metadata only.
_Avoid_: viewer, editor

**Managed skill**:
A skill with a lock entry, whether its files live in the canonical store or only in agent copies.
_Avoid_: installed skill, tracked skill

**Unmanaged skill**:
A skill found on disk without a lock entry.
_Avoid_: foreign skill, external skill

**Copy-only skill**:
A managed skill with no canonical folder; its files exist only as agent copies.
_Avoid_: legacy skill, detached skill

**Stale lock entry**:
A lock entry with no corresponding skill folder anywhere.
_Avoid_: ghost, orphan

**Source**:
The upstream origin a skill is installed from; v1 accepts public GitHub repositories.
_Avoid_: repo, registry

**Install**:
Add a skill from a source to the canonical store and attach it to the selected agents.
_Avoid_: add, sync

**Install preview**:
The read-only list of skills discovered in a source before installation.
_Avoid_: scan, listing

**Install target**:
The selected agents plus the canonical store that an install writes to.
_Avoid_: destination, target agents

**Agent**:
A coding agent with its own skills directory for each scope, such as Claude Code or Codex.
_Avoid_: client, target

**Extra agent**:
An agent defined outside the vendored CLI catalog, either shipped with the app or added by the user; skills-box attaches its skills natively instead of through the CLI. It appears in the UI only when its detection path exists, and participates in project scope only when a project skills directory is configured.
_Avoid_: custom agent, third-party agent

**Agent attachment**:
The representation of a skill inside an agent's skills directory for one scope, either a symbolic link or a copy. The Chinese UI labels it 「挂载」.
_Avoid_: installation, mount

**Attach**:
Create a skill's attachment in one or more agents without reinstalling, keeping the canonical store and lock entry intact. It is the inverse of detach and requires the canonical folder.
_Avoid_: mount, enable

**Detach**:
Remove a skill from one agent while keeping the canonical store and the skill's lock entry intact.
_Avoid_: uninstall, disable, delete

**Uninstall**:
Remove a skill's canonical folder, all agent attachments, and its lock entry.
_Avoid_: remove, delete

**Lock entry**:
The record that ties a skill name to its source, path, hash, and install/update times.
_Avoid_: manifest, metadata file

**Update check**:
The read-only comparison of each managed skill's recorded version fingerprint against its source, run automatically or on demand for one scope. It never changes skill files.
_Avoid_: update, sync, refresh

**Check attempt**:
One run of an update check, whether automatic or manual. A run that only uses cached source data is still a check attempt.
_Avoid_: scan, poll

**Last check**:
The recorded time of the most recent check attempt for one scope. A check scoped to one skill does not advance it.
_Avoid_: last sync, last refresh

**Last upstream success**:
The most recent time a source was successfully read from upstream. Cache hits do not advance it.
_Avoid_: last check, last fetch

**Update**:
Refresh a skill's canonical folder from its source and re-establish its attachments. Manually triggered by the user after an update check; never automatic.
_Avoid_: auto-update, sync, upgrade

**Update candidate**:
A skill whose upstream version fingerprint differs from the one recorded in its lock entry.
_Avoid_: outdated skill, dirty skill

**Update target**:
The normalized GitHub source and optional safe ref resolved from a lock entry; update checks and update execution share it.
_Avoid_: source spec, repo ref

**Local modification**:
A difference between a skill's current files and the baseline recorded at its last install or update.
_Avoid_: dirty copy, user edit

**Baseline fingerprint**:
The recorded fingerprint of a skill folder used to detect local modifications.
_Avoid_: hash, checksum

**Backup snapshot**:
A pre-operation copy of the lock, canonical folder, copy attachments, and attachment manifest.
_Avoid_: archive, dump

**Read model**:
The projection of the lock and filesystem that the UI displays.
_Avoid_: cache, state

**Mutation task**:
A serialized operation that changes skills or agent attachments.
_Avoid_: job, command

**Mutation session**:
One execution of one mutation task, ending in a single outcome. The task is the serialized operation; the session is one run of it.
_Avoid_: job, runner, transaction

**Mutation surface**:
The shared frontend lifecycle for every mutation dialog: drivers normalize CLI task streams and native promises into one outcome, progress/error/result UI and refresh policy.
_Avoid_: task runner, mutation store, dialog runner

**Fake backend**:
The typed in-memory implementation of every IPC command shared by App tests and the preview; it owns call recording, task channels and native snapshot changes.
_Avoid_: mock, stub, test server

**Anomaly**:
A state that does not match the canonical model, such as an orphan folder, broken link, or stale lock entry; v1 surfaces it read-only.
_Avoid_: error, corruption

**Scope**:
Whether a skill belongs to the global store or to one registered project.
_Avoid_: level, target

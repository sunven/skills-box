# Skills Box

A macOS-first desktop tool for managing globally installed Agent Skills across coding agents: inventory, install, update, detach, and uninstall.

## Language

**Global skill**:
A skill installed at user scope, available across projects and mounted into one or more agents.
_Avoid_: user skill, system skill

**Canonical store**:
The single directory that holds the actual skill folder shared by all agents.
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
A coding agent with its own global skills directory, such as Claude Code or Codex.
_Avoid_: client, target

**Extra agent**:
An agent defined outside the vendored CLI catalog, either shipped with the app or added by the user; skills-box attaches its skills natively instead of through the CLI. It appears in the UI only when its detection path exists.
_Avoid_: custom agent, third-party agent

**Agent attachment**:
The representation of a global skill inside an agent's skills directory, either a symbolic link or a copy. The Chinese UI labels it 「挂载」.
_Avoid_: installation, mount

**Attach**:
Create a global skill's attachment in one more agent without reinstalling, keeping the canonical store and lock entry intact. It is the inverse of detach and requires the canonical folder.
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
The read-only comparison of each managed skill's recorded tree hash against its source, run automatically or on demand. It never changes skill files.
_Avoid_: update, sync, refresh

**Check attempt**:
One run of an update check, whether automatic or manual. A run that only uses cached source trees is still a check attempt.
_Avoid_: scan, poll

**Last check**:
The recorded time of the most recent check attempt over all managed skills. A check scoped to one skill does not advance it.
_Avoid_: last sync, last refresh

**Last upstream success**:
The most recent time a source was successfully read from upstream. Cache hits do not advance it.
_Avoid_: last check, last fetch

**Update**:
Refresh a skill's canonical folder from its source and re-establish its attachments. Manually triggered by the user after an update check; never automatic.
_Avoid_: auto-update, sync, upgrade

**Update candidate**:
A skill whose upstream tree hash differs from the hash recorded in its lock entry.
_Avoid_: outdated skill, dirty skill

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

**Anomaly**:
A state that does not match the canonical model, such as an orphan folder, broken link, or stale lock entry; v1 surfaces it read-only.
_Avoid_: error, corruption

**Scope**:
Whether a skill is global (user-level) or project-level; v1 manages global scope only.
_Avoid_: level, target

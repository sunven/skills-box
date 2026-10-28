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

**Agent attachment**:
The representation of a global skill inside an agent's skills directory, either a symbolic link or a copy.
_Avoid_: installation, mount

**Detach**:
Remove a skill from one agent while keeping the canonical store and the skill's lock entry intact.
_Avoid_: uninstall, disable, delete

**Uninstall**:
Remove a skill's canonical folder, all agent attachments, and its lock entry.
_Avoid_: remove, delete

**Lock entry**:
The record that ties a skill name to its source, path, hash, and install/update times.
_Avoid_: manifest, metadata file

**Update**:
Refresh a skill's canonical folder from its source and re-establish its attachments, manually triggered by the user.
_Avoid_: sync, upgrade

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

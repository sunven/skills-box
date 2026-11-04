# App updates own installation and restart in Rust

Status: accepted

App update replaces Skills Box itself, while Update changes a Skill in one Scope. A direct renderer updater/process flow is convenient, but it cannot prove that unregistered native writes and blocking planning have finished, and Tauri restart cannot be vetoed by the existing exit handler. Rust therefore owns the App update handle, verified download bytes, installation worker and controlled shutdown/restart, acquiring the existing global mutation permit only for installation and retaining it through the restart handoff; a separate application installation state also guards close/quit and writes that normally live outside the permit.

This preserves ADRs 0013 and 0016: download does not serialize unrelated Skill writes, and dropping an IPC request cannot release a worker's live permit. App update does not enter the Skill task registry or produce Skill interruption markers. Public stable Releases use this application's own updater signing key, independently of Skill source credentials and the existing macOS ad-hoc signing policy. The product boundaries and complete specification were confirmed on 2026-11-03, and the user authorized implementation.

On 2026-11-04, the user required automatic releases to strictly follow `/Users/sunven/github/fanyifanyi`. The release workflow therefore uses `tauri-apps/tauri-action@v1` to build and publish one Apple Silicon target directly, generate `latest.json`, then check its updater metadata. This supersedes the earlier dual-architecture draft-release gate; local release preparation follows the reference version-bump, git-cliff changelog, commit, annotated-tag and confirmed-push sequence.

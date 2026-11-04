# Extra agents attach natively

Status: accepted

[ADR-0023](0023-uninstall-dependency-boundary.md) also represents Codex's existing legacy skills directory through this overlay. That compatibility entry is detected by the skills directory itself and hidden when it is absent.

Agents outside the vendored `skills@1.7.4` catalog — WorkBuddy (`~/.workbuddy/skills`) is the first — follow the same on-disk convention as catalog agents, but the CLI rejects unknown agent ids and has no arbitrary-directory option, so it cannot create their attachments. skills-box therefore defines **extra agents** as its own data (builtin entries shipped with the app plus user-defined entries in the app-data `agents.json`, detected by the parent of their skills directory) and attaches their skills natively: the CLI still installs into the canonical store and links catalog agents, then skills-box creates a symlink from the extra agent's skills directory into `~/.agents/skills/<name>` itself. WorkBuddy indexes symlinked skill folders (verified against its skill-list cache), so no copy mode is needed.

## Considered options

- **Upstream each agent to vercel-labs/skills**: rejected as the only path because the catalog is a static list and PR timing is outside our control; it remains a fine supplement.
- **Copy attachments for extra agents**: rejected because it duplicates state, breaks the canonical-store model, and WorkBuddy follows symlinks.
- **Fold builtin extras into `catalog.json`**: rejected because that file is vendored and replaced on CLI bumps.

## Consequences

- The CLI is no longer the only attachment writer; ADR 0001's "CLI mutations, native reads" holds for catalog agents only. Attach, detach, extra-agent verification, and extra-link cleanup after update and uninstall are native.
- `catalog.json` and its 79-agent assertion stay untouched; extras live in a separate overlay merged at load.
- Extra agents are invisible to `npx skills` outside skills-box; a skill attached to one shows no CLI agent link.
- Detecting a new extra agent does not attach existing skills; the user does that through the native attach action.

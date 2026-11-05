# Skills Box

A macOS-first desktop app for managing globally installed Agent Skills across
coding agents: inventory, discover, install, update, detach, and uninstall.

Install from public HTTPS Git repositories, including GitHub, GitLab, Gitee,
CNB and self-hosted services. GitHub also accepts `owner/repo` shorthand.
For example, enter `https://cnb.cool/cnb/skills/cnb-skill` in the install
wizard; Skills Box supplies `.git` for `skills@latest`. Installation uses the
default branch, and update checks work at both global and project scope.

## CLI command display

Source previews, Install, Update, Uninstall and their retries show the backend's
command before execution. Commands include the actual working directory and
absolute npx path: `cd <cwd> && <npx> -y skills@latest …`. If the command changes
before a write, the app refreshes the preview and requires another click.
Running and completed views retain the actual invocations for the session.
Native actions and skipped targets have no invented CLI command.

Environment diagnostics show the latest actual version probe; each source's
discovery scan record shows its latest actual catalog call with a timestamp.
Cache hits do not create new invocation records. These records are kept only
in memory and disappear when the application exits.

## GitHub authentication

Open **设置 → GitHub 认证** to save or replace a personal access token, delete
it, or test the active authentication and inspect the remaining API quota.
The token applies to native GitHub API requests for global and project update
checks and source discovery.
Public repository access does not require private repository permissions.

Credential precedence is **saved token → `GITHUB_TOKEN` → anonymous**.
Saving or deleting takes effect for subsequent requests without restarting.
`GITHUB_TOKEN` is read from the application's inherited startup environment;
changing it requires restarting the app. Shell profiles are not executed to
discover credentials. When launching from Finder, configure the token in the
settings page if the application does not inherit the variable.

Saved credentials live in
`~/Library/Application Support/com.sunven.skills-box/settings.json` as
plaintext with Unix `0600` permissions. They are not returned by the settings
read API or forwarded to Git/CLI subprocesses. An invalid token produces an
authentication error rather than silently trying another credential.

## Discover upstream skills

Open **发现** to browse skills from the public Git sources already used by
Global and registered Projects. Sources are deduplicated across scopes and
follow their default branch; existing update checks still follow the lock ref.
The first successful observation establishes a baseline without a “新增” badge.
Names first observed afterward create one application-wide unread reminder.

The app checks due sources at startup and every six hours while running. New
active sources are checked immediately. **立即检查上游** bypasses the six-hour
interval, reuses a valid catalog when the commit is unchanged, and respects
GitHub rate-limit recovery. Sources run in batches of 20 with at most four
concurrent scans. Failures preserve the last catalog and successful check time.

The list defaults to skills not installed in the selected Scope. **查看全部新增**
shows every unread item counted by the header, including items installed in
that Scope. Viewing details, successful installation, or **全部已读** handles a
reminder. Ignored items can be restored and remain read. These decisions survive
restarts and temporary source inactivity in app-data `discovery.json`. A corrupt
history file is reported and preserved; restore it before restarting the app.

**安装** reopens the existing wizard, previews the source again and selects only
the clicked skill. Choose a Scope and agents and confirm before anything is
installed. A vanished or ambiguous cached item requires rechecking the source.

Initial discovery and changed source versions use the CLI's read-only
preview and require Node/npx/Git. GitHub version queries use the configured
token; CLI-owned requests do not receive it and can use their anonymous
fetch/clone path. Discovery has no manual source subscriptions, system
notifications, background scans after exit, or automatic installation/update.

## App updates

Open **设置 → 应用更新** to check the current application version, read release
notes and install a newer stable release. Production builds check one second
after startup and every six hours while running; development builds do not
contact the update service. App updates are independent of Skill update checks
and do not use the configured GitHub token.

Clicking **更新到 …** downloads and verifies the update, then installs and
restarts. Skill operations can continue during download. If a write is still
running when the download finishes, the app retains that download for the
session and asks you to click **安装并重启** when ready. Installation prevents
new writes and closing/quitting until the restart handoff. No Skill files are
changed by an App update.

Automatic releases follow fanyifanyi and target macOS Apple Silicon, with
ad-hoc application signing. When opening a download for the first time, macOS may
require approval in Privacy & Security. Move the app from its DMG to Applications
or another writable directory before updating. See [release setup](docs/releasing.md)
for signing keys, local signed builds and the complete release workflow.

## Development

```bash
pnpm install
pnpm tauri dev
```

## Checks

```bash
pnpm test        # Vitest + React Testing Library (mockIPC)
pnpm build       # tsc + vite production build
cd src-tauri && cargo test
# Optional real CNB lifecycle in an isolated temporary HOME (requires network):
cargo test --test generic_git_sources -- --ignored --nocapture
```

## Context

- Product spec: `.scratch/global-skill-manager/spec.md`
- Source discovery spec: `.scratch/skill-discovery/spec.md`
- Auto update check spec: `.scratch/auto-update-check/spec.md`
- App update spec: `.scratch/app-update/spec.md`
- Domain glossary: `GLOSSARY.md`
- Architecture decision: `docs/adr/0001-cli-mutations-native-reads.md`

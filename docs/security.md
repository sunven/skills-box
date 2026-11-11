# Security boundaries (v1)

This file records the final hardening decisions for Skills Box v1. It is the
reference for release review and for ticket 11 (acceptance).

## Application update boundary (2026-11-03)

App updates use a separate public stable Release and this application's own
updater signing key. The renderer receives version metadata, progress and
normalized errors through three typed commands; it never receives update
handles, private keys or downloaded bytes. Updater and process restart
permissions are not granted to the renderer. Skill-source tokens are not sent
to the update service.

Rust verifies download signatures and their signed application versions before
installation, owns the blocking installation
worker and retains the shared mutation permit through controlled restart.
Configuration writes and ordinary exit use an additional read/write gate, so
they cannot race with application replacement. The updater also shuts down
and reaps read-only preview work before restarting. Busy installation never
force-cancels Skill writes. Release signing does not change the existing macOS
ad-hoc/no-notarization policy.

The committed configuration contains only the public key. Private key and
password files remain outside the repository with `0600` permissions; the
local build helper and Actions use them through process environment variables.
Following the reference workflow, the official Tauri action builds and signs
the Apple Silicon application, publishes the Release and generates its updater
manifest. A subsequent step checks the published metadata; this is a
post-publication check, not a draft-release validation gate. Rust still verifies
the signature and signed version before installing an update. See
[release setup](releasing.md) for the remaining remote prerequisites.

## Renderer permissions

- Tauri capabilities expose **only** `core:default` and `dialog:allow-open`
  for the `main` window (`src-tauri/capabilities/default.json`). The dialog
  plugin is used by project registration and batch project destination selection; no save,
  message or ask dialog is allowed. There is no `shell`, `fs` or `opener`
  permission in the frontend.
- `add_project` / `relocate_project` accept the folder chosen by that picker.
  Batch Copy to project also accepts an explicitly selected `projectRoot` for
  a bounded read-only preview. These path inputs use the same validation:
  Rust canonicalizes, deduplicates and rejects
  `/`, the user home and the global canonical store, and the resulting root is
  the only extra root ordinary content reads may reach. Batch copying validates
  the pending root without registration or baseline writes, and registers it
  only after confirmation under the write permit. Project content, links and
  lock paths stay physically within that root; copies reject absolute, external
  and broken internal links. New directories are published without replacement.
- The opener is used only from Rust commands: `open_skill_folder`,
  `open_skill_file`, `reveal_app_data` (server-resolved paths guarded by
  `security::ensure_allowed`) and `open_external_url` (absolute `https://` with
  a host, re-validated in Rust). The frontend never receives a path or argv.
- `opener:default` was removed from the capability because it also allows
  `mailto:`, `tel:` and `http:` URLs. The plugin stays registered for Rust use.

## Skill content reads

`list_skill_dir` and `read_skill_file` accept only a skill id and a relative
path. Rust resolves the skill root from the inventory, rejects absolute paths,
`.`/`..`, empty segments, backslashes, NUL, and the hidden `.git`,
`node_modules` and `.DS_Store` components, canonicalizes the target, and
rejects anything that resolves outside the skill root. Listings cap at 500
entries (2,000 scanned) and depth 12; text previews cap at 1 MiB and images at
5 MiB. Image previews travel as base64 data URLs; remote URLs are never
fetched.

## CSP

Final production policy (`src-tauri/tauri.conf.json`):

```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
img-src 'self' data:; font-src 'self' data:;
connect-src 'self' ipc: http://ipc.localhost;
object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'
```

- No remote script, image, font, frame or object origin is allowed.
- **Tailwind / inline-style exception:** `style-src 'unsafe-inline'` is required
  because Tailwind v4 plus React inline styles drive dynamic layout (for
  example the task progress bar width and avatar colours). Inline **scripts**
  remain forbidden; `unsafe-eval` is never allowed. The renderer still uses
  `skipHtml` + `rehype-sanitize` for SKILL.md, so a Tailwind style exception
  cannot execute Markdown content.
- `devCsp` only adds the Vite dev server on `localhost:1420`; production CSP is
  unchanged.
- Remote images in SKILL.md render as text/links and are never fetched; links
  are https-only and leave through the Rust opener command.

## CLI process boundary

- No shell is used. `TokioExecutor` spawns `Command::new(program).args(...)`
  with `env_clear()` and a minimal allowlist, `stdin(Stdio::null())`, and a
  dedicated process group so cancel/timeout can `SIGTERM` then `SIGKILL` the
  whole npx → npm → git tree. The deadline and cancellation cover output
  collection as well as the direct child, including descendants that retain
  output pipes after that child exits.
- Sources, skill names, agent ids, `skillId` and app-data kinds are validated
  server-side. The renderer sends only semantic values (`source`, skill ids,
  agent ids, `taskId`, app-data `kind`, an https URL); **paths never cross IPC
  as inputs**. Results may echo display paths for the UI, but no command
  accepts a path, argv, env, executable or URL other than the re-validated
  external-link command.
- `skills list -g --json` is diagnostic-only and is not implemented as a data
  source. The app never spawns `skills list`, `skills update`, `skills check`,
  `remove -a`, `remove --all`, `add --all`, `-a '*'` or `--copy`.
- Mutations are serialized and create a lock snapshot before writing.
- Project mutation readiness validates the physical containment of the
  canonical store and lock, including existing parent links for missing paths.
  Git installs repeat these checks before backup and immediately before the
  CLI runs. Uninstall rejects a linked Agent skills directory, and copy
  dependency checks protect links to both the copy and its contents.
- Ordinary shutdown waits for unregistered native workers to release the
  mutation permit, including workers whose requesting future was dropped.
  These workers settle their file operations before the application exits.
- Command previews accept the same semantic requests as task starts. Rust
  formats the executable, cwd and argv from the validated execution plan and
  returns copyable text plus a preview fingerprint. Starts revalidate that
  fingerprint under the mutation permit, then compare the actual command
  again before invocation. The renderer cannot submit a command to execute.
- Session-only `cli-command` events expose command text, context and invocation
  status, never the child environment or credentials. Environment probes and
  discovery calls share the same observation boundary. Native operations,
  cache hits and targets cancelled or skipped before invocation emit no call.

## GitHub authentication

- Rust owns the shared settings service. Saved credentials take precedence
  over the application's inherited `GITHUB_TOKEN`, followed by anonymous
  access. The startup environment is captured once without executing a shell.
- Settings reads, saves and deletions return only presence flags and the
  effective credential source. Token drafts cross IPC only when saved; they
  are not retained in frontend query/mutation caches, localStorage, debug
  output, operation logs or the fake backend's recorded calls.
- `settings.json` stores plaintext credentials in the application data
  directory. Both the final file and the temporary replacement have Unix
  `0600` permissions before any token is written. Writes use an atomic rename;
  failed writes retain the previous file and in-memory credentials. Reads
  reject symlinks and non-regular files, cap file size, and repair loose file
  permissions. Invalid configuration reports a repairable error and never
  silently falls back to another credential.
- Authentication is attached per request, solely to `api.github.com`, for
  Trees API, commit lookup, and `/rate_limit`. The Authorization header is
  marked sensitive, API redirects are disabled, and error bodies are redacted.
  Codeload uses a separate uncredentialed transport; Git and CLI child
  environments continue to exclude `GITHUB_TOKEN` and `GH_TOKEN`.
- HTTP 401 reports invalid/expired credentials without retrying another
  token. Authentication tests and settings operations require no new
  renderer permissions or remote CSP origins.

## Debug-only test hooks

`SKILLS_BOX_ROOT`, `SKILLS_BOX_NODE` and `SKILLS_BOX_FAKE_CLI` are honored only
in debug builds:

- `SKILLS_BOX_ROOT` redirects app data only (never `HOME`).
- `SKILLS_BOX_NODE` overrides Node discovery and disables fallback probing.
- `SKILLS_BOX_FAKE_CLI` rewrites only invocations of the skills CLI
  (`slow[:seconds]`, `fail`, `invalid-json`, `cancel`) inside the real
  `TokioExecutor`, preserving timeout and process-group cancellation. Release
  builds compile the check out (`cfg!(debug_assertions)` is false).
- **`/bin/sh` exception:** the fake-CLI rewrite is the only place a shell is
  spawned (`/bin/sh -c <fixed script>`), and it exists solely in debug builds
  and only for skills CLI invocations such as `skills@latest`. Node/git probes, every
  production mutation and every other command are never shell-wrapped. The
  script text is a fixed constant selected by the env value; no user, source,
  skill name or path is interpolated into it. Release builds cannot reach
  `apply_fake_cli` with `debug = false`, so the release binary has no shell
  execution path.


## Source discovery

- Discovery reads only Global and currently registered Project installation
  records. A retained snapshot is usable only at the same Project revision;
  Forget/Relocate revoke it in memory even if auxiliary persistence fails.
- Source network/CLI work runs without a Mutation permit. The shared preview
  seam uses `skills@latest add <source> -l [-g]` and explicitly removes GitHub token
  variables from subprocess environments. Native tokens stay on the GitHub
  REST origin. No remote file browser or automatic installation is added.
- `discovery.json` stores catalog/history, scope-source snapshots, and a
  credential fingerprint for rate-limit context, never the token. Reads are
  bounded at 32 MiB and reject unsupported/corrupt data and symlinks. Writes
  use an atomic rename and mode `0600`. Failed writes do not acknowledge
  reminders or claim that ignore decisions were saved.
- Application exit cancels queued work and the read-only process groups. Source
  generation and credential revisions prevent late requests from publishing
  into a revoked association or setting a new credential's cooldown.

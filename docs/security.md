# Security boundaries (v1)

This file records the final hardening decisions for Skills Box v1. It is the
reference for release review and for ticket 11 (acceptance).

## Renderer permissions

- Tauri capabilities expose **only** `core:default` and `dialog:allow-open`
  for the `main` window (`src-tauri/capabilities/default.json`). The dialog
  plugin is used solely by the project-registration folder picker; no save,
  message or ask dialog is allowed. There is no `shell`, `fs` or `opener`
  permission in the frontend.
- `add_project` / `relocate_project` accept the folder chosen by that picker.
  They are the only path inputs: Rust canonicalizes, deduplicates and rejects
  `/`, the user home and the global canonical store, and the resulting root is
  the only extra root content reads may reach.
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
  whole npx → npm → git tree.
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
  Codeload uses a separate uncredentialed transport; Git and pinned CLI child
  environments continue to exclude `GITHUB_TOKEN` and `GH_TOKEN`.
- HTTP 401 reports invalid/expired credentials without retrying another
  token. Authentication tests and settings operations require no new
  renderer permissions or remote CSP origins.

## Debug-only test hooks

`SKILLS_BOX_ROOT`, `SKILLS_BOX_NODE` and `SKILLS_BOX_FAKE_CLI` are honored only
in debug builds:

- `SKILLS_BOX_ROOT` redirects app data only (never `HOME`).
- `SKILLS_BOX_NODE` overrides Node discovery and disables fallback probing.
- `SKILLS_BOX_FAKE_CLI` rewrites only invocations of the pinned skills CLI
  (`slow[:seconds]`, `fail`, `invalid-json`, `cancel`) inside the real
  `TokioExecutor`, preserving timeout and process-group cancellation. Release
  builds compile the check out (`cfg!(debug_assertions)` is false).
- **`/bin/sh` exception:** the fake-CLI rewrite is the only place a shell is
  spawned (`/bin/sh -c <fixed script>`), and it exists solely in debug builds
  and only for the pinned `skills@1.7.4` invocation. Node/git probes, every
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
  seam uses pinned `add <source> -l [-g]` and explicitly removes GitHub token
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

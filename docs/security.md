# Security boundaries (v1)

This file records the final hardening decisions for Skills Box v1. It is the
reference for release review and for ticket 11 (acceptance).

## Renderer permissions

- Tauri capabilities expose **only** `core:default` for the `main` window
  (`src-tauri/capabilities/default.json`). There is no `shell`, `fs`, `dialog`
  or `opener` permission in the frontend.
- The opener is used only from Rust commands: `open_skill_folder`,
  `open_skill_md`, `reveal_app_data` (server-resolved paths guarded by
  `path::ensure_allowed`) and `open_external_url` (absolute `https://` with a
  host, re-validated in Rust). The frontend never receives a path or argv.
- `opener:default` was removed from the capability because it also allows
  `mailto:`, `tel:` and `http:` URLs. The plugin stays registered for Rust use.

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
  server-side. Paths never cross IPC.
- `skills list -g --json` is diagnostic-only and is not implemented as a data
  source. The app never spawns `skills list`, `skills update`, `skills check`,
  `remove -a`, `remove --all`, `add --all`, `-a '*'` or `--copy`.
- Mutations are serialized and create a lock snapshot before writing.

## Debug-only test hooks

`SKILLS_BOX_ROOT`, `SKILLS_BOX_NODE` and `SKILLS_BOX_FAKE_CLI` are honored only
in debug builds:

- `SKILLS_BOX_ROOT` redirects app data only (never `HOME`).
- `SKILLS_BOX_NODE` overrides Node discovery and disables fallback probing.
- `SKILLS_BOX_FAKE_CLI` rewrites only invocations of the pinned skills CLI
  (`slow[:seconds]`, `fail`, `invalid-json`, `cancel`) inside the real
  `TokioExecutor`, preserving timeout and process-group cancellation. Release
  builds compile the check out (`cfg!(debug_assertions)` is false).

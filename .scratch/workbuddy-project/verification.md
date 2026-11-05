# WorkBuddy project targets

Verified on 2026-11-05.

WorkBuddy was absent from project Install and Attach targets because its builtin
extra-agent definition supplied only `~/.workbuddy/skills` and no project skills
directory.

## Independent project directories

After confirming the desktop host's native directory reader, the user requested
independent directories on 2026-11-05. WorkBuddy uses `.workbuddy/skills` with id
`workbuddy` and label `WorkBuddy`; CodeBuddy uses `.codebuddy/skills` with id
`codebuddy` and label `CodeBuddy`. Inventory scans each directory separately and
reports each Agent's attachment. A detected WorkBuddy is eligible before its
project directory exists.

Install writes the canonical store through the CLI and then creates WorkBuddy's
attachment natively. The CLI command uses `skills@latest` and never receives the
unknown `workbuddy` id. Attach and Detach operate natively on WorkBuddy's own
project link. Global targets are `~/.workbuddy/skills` and `~/.codebuddy/skills`.

## WorkBuddy directory readers

The user subsequently reported that a project's `.workbuddy` directory also
works. Reinspection of the installed WorkBuddy 5.9.3 bundle confirmed two
different readers:

- Desktop host: `main/server.js` inside `app.asar`, `scanSkillListOnce`, scans
  `join(cwd, WORKBUDDY_DATA_FOLDER_NAME || ".workbuddy", "skills")` with Project
  origin. Its skill management root uses the same directory. This is native
  support for `.workbuddy/skills` without a project config override.
- Bundled CodeBuddy CLI:
  `app.asar.unpacked/cli/dist/codebuddy-lite-wb.mjs`,
  `PathUtils.getProjectHomeDirFor()` and `getProjectHomeDir()`, resolve the
  project directory from `CODEBUDDY_PROJECT_CONFIG_DIR`, then
  `dataLayout.configDir.projectDirName`, then the `.codebuddy` fallback. Its
  skill loader appends `skills` to that path.

The earlier statement that `.workbuddy/skills` requires
`CODEBUDDY_PROJECT_CONFIG_DIR=.workbuddy` was incomplete: it inspected only the
bundled CLI and omitted the desktop host reader. The independent WorkBuddy
project directory matches the desktop host reader without a project config
override.

## Validation

- Rust coverage verifies builtin paths, independent detection, a missing
  project directory, native WorkBuddy Install attachments after canonical CLI
  Install, separate inventory attachments and native Attach/Detach preserving
  CodeBuddy's project attachment and both Agents' global links and skill content.
- Copy to project UI coverage exercises WorkBuddy alone and both Agents
  detected: independent checkboxes, distinct preview and result paths, and
  separate global Agent rows.
- `CARGO_INCREMENTAL=0 cargo test`: 658 passed (625 library and 33 integration
  tests); four network tests remain explicitly ignored. The initial compilation
  ran out of disk space while writing an incremental query cache. Removing the
  failed crate's generated incremental cache and disabling incremental output
  for this test run resolved it.
- `pnpm test --maxWorkers=4`: 322 passed across 37 files.
- `pnpm build`, `cargo fmt --all --check`, and `git diff --check`: passed. The
  existing Vite chunk-size warning remains.

The existing [desktop](desktop.png) and [mobile](mobile.png) screenshots are from
the earlier independent-directory implementation and show the same WorkBuddy
project path. Current behavior is verified by the automated tests above. No
user Skill files or third-party runtime configuration were changed during
verification.

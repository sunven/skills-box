# Changelog

All notable changes to this project will be documented in this file.

## [0.1.4] - 2026-11-09

### Bug Fixes

- Refine skill inventory controls and filtered counts

### Features

- Improve install skill selection and disable input suggestions
- Unify CLI command help tooltips
- Improve install wizard guidance and source history
- Add application data management settings

### Refactor

- Isolate batch target execution and state
- Isolate batch operation preview and retry flow
- Share update check attempt scheduling

## [0.1.3] - 2026-11-06

### Bug Fixes

- Use Skills Box as the application name

### Features

- Add common actions to skill list rows
- Add skill multi-select and batch operations
- Copy global skills to a project from batch attach
- Select agents for batch skill removal
- Preview batch update commands and refine batch controls
- Display actual skills CLI commands and use latest version
- Support independent WorkBuddy project skills
- Clean stale records and restore missing skills

### Refactor

- Consolidate duplicated helpers and shared UI state

## [0.1.2] - 2026-11-04

### Miscellaneous Tasks

- **release**: Follow fanyifanyi release workflow

## [0.1.1] - 2026-11-03

### Bug Fixes

- **integration**: Exclude .qoder agent worktrees from vitest discovery
- Trap and restore focus in the anomaly dialog (ticket 02)
- **ticket 03**: Keep copy-only detail Finder-only
- **ticket 04**: Escalate cancellation to SIGKILL for lingering groups
- **ticket 05**: Keep agent choices stable and label universal shared target
- **ticket 05**: Require selected agents to be detected server-side
- **ticket 06**: Harden cache writes and HTTP error details
- **ticket 07**: Preserve cancel results and filter universal agents in confirm UI
- **review**: Complete P0 refresh, anomaly, baseline, retry, cancel, stale-lock fixes
- **review**: Share one task runner across mutation dialogs and close contract gaps
- **extras**: Reject builtin edits before project guard
- **uninstall**: Use the scope-qualified skill id and project command copy
- **workspace**: Recognize lock.corrupt and expose inactive lock errors
- **update**: Always check once at launch, then honor the six-hour window
- **projects**: Guard registry changes and isolate install lock snapshots
- **scope**: Require explicit scope and add the install wizard shortcut
- **ui**: Repair project scope rail layout
- **projects**: Serialize root changes with the mutation permit
- **scope**: Inspect the crashed scope's lock on restart
- **react**: Refresh content cache and avoid redundant renders

### Documentation

- **acceptance**: Record ticket 11 results, rebuilt app, and sign-off gaps
- Resolve implementation tickets 01-11
- **project-scope**: Add spec, issues and ADRs
- **spec**: Cite the project sandbox in the acceptance matrix
- **update**: Record the non-ASCII content-hash limitation
- **project-scope**: Resolve tickets and record follow-ups

### Features

- Add environment detection backend for app shell (ticket 01)
- Build Chinese three-column shell with environment banner (ticket 01)
- Add strict inventory read model and flat skill list (ticket 02)
- **ticket 04**: Align preview phases and default skill selection
- **ticket 05**: Install execution with CLI JSON parsing and verification
- **ticket 05**: Wizard agent selection, confirm, and install results
- **ticket 06**: Read-only GitHub update checker with cache
- **ticket 06**: Update badges, filters, anomaly counts and update-all summary
- **ticket 07**: Add backup snapshot store and JSONL operation log
- **ticket 07**: Plan and execute serialized skill updates with backups
- **ticket 07**: Expose start_update and reveal_app_data commands
- **ticket 07**: Add confirm, progress, result and retry UI for updates
- **detach**: Unlink symlink attachments without touching canonical or lock
- **detach**: Back up and remove copy attachments with realpath dependency guard
- **detach**: Serialize detach through the single-writer queue and expose the command
- **detach**: Add detail-pane detach entry, confirmation and blocked states
- **ticket 09**: Plan uninstall, build remove argv and verify removal
- **ticket 09**: Execute serialized uninstall with backup and verification
- **ticket 09**: Add the danger-confirm uninstall dialog and action
- **hardening**: Add debug fake CLI injection and enforce the timeout matrix
- **hardening**: Single instance, exit protection, and interruption verification
- **security**: Route external links through Rust and lock down CSP, capabilities, and CLI flags
- **ui**: Share modal focus handling and surface exit and interrupted-mutation warnings
- **ui**: Rebuild the workspace on the achromatic ledger system
- **ui**: Show local brand logos for agents
- **ui**: Open skill details in an on-demand drawer
- **ui**: Browse full skill content in a tabbed drawer
- **updates**: Auto-check skills and persist the last check
- **ui**: Clamp long skill descriptions to three lines
- **agents**: Support agents outside the pinned CLI catalog
- **projects**: Add project registry and scope plumbing
- **projects**: Read project inventory, locks and content
- **scope**: Add scope switcher and project registry UI
- **update**: Add content fingerprints, archive cache and v2 per-scope state
- **update**: Check project locks by CLI content fingerprint
- **update**: Scope auto-checks and fake backend state per scope
- **install**: Run project installs, attachments and previews in scope
- **update**: Run project updates and uninstalls in scope
- **extras**: Manage project skills dirs with guards
- **mutation**: Add a preview mode to the mutation surface
- Add Git sources, token settings, and upstream skill discovery
- Move scope and filters to toolbar and organize settings
- Show installable skills with source and scope filters
- Add signed application updates for macOS
- Add release preparation script

### Miscellaneous Tasks

- Ignore .qoder agent worktrees
- **build**: Ignore the shared cargo target symlink
- Configure updater signing key

### Performance

- **ui**: Lazy-load skill content and cut avoidable re-renders

### Refactor

- **detach**: Expose the copy dependency scan for uninstall reuse
- **mutations**: Run CLI mutations through one session
- **read-model**: Own the catalog and per-skill views
- **attachments**: Centralize attachment target eligibility
- **errors**: Unify the error taxonomy across the IPC seam
- **ui**: Replace App memos with one workspace projection
- **mutations**: Unify task state, results and refresh
- **update**: Share target resolution and tree hashing
- **ui**: 将详情操作移至顶部
- Dedupe UI components and backend helpers
- **scope**: Unify the scope lock behind ScopedLock
- **scope**: Resolve every scope through one resolver
- **update**: Pass the scope layout to the project check
- **update**: Share one check-state store
- **update**: Type the unknown update reason
- **update**: Return the persisted state and batch scope reads
- **tasks**: Build the task result from a registered envelope
- **tasks**: Share the add-output mapping and unit failure bookkeeping
- **tasks**: Take install snapshots through BackupStore
- **mutation**: Carry the result scope and split refresh policy
- **projects**: Move registry dialogs onto the mutation surface
- **install**: Fold preview into the mutation surface
- Deduplicate persistence helpers and view state
- **updates**: Centralize project check lifecycle
- **tasks**: Centralize scope mutation lifecycle
- **cli**: Derive execution scope from layout
- **paths**: Centralize project physical path checks
- Remove redundant state and reuse shared helpers
- **update**: Encapsulate source archive acquisition
- **mutation**: Centralize close and cancel lifecycle

### Security

- **acceptance**: Ad-hoc sign the macOS bundle at macOS 12 minimum

### Styling

- **ticket 03**: Format new Rust detail, baseline, and security modules
- **ticket 05**: Format install module and fix clippy suggestion
- **ticket 07**: Tidy update execution test formatting
- **project-scope**: Apply rustfmt after scope merges
- Display environment metrics in two columns

### Testing

- **ticket 04**: Pin the TaskEvent JSON contract
- **ticket 06**: Cover ref grouping, root skills and missing files
- **ticket 07**: Cover universal-agent filtering and cancelled update results
- **detach**: Cover rejections, path guards, backup failure and read-model eligibility
- **detach**: Lock the read-model and error-code contracts; format detach module
- **hardening**: Prove fake-slow timeout, cancel lock integrity, and injection rejection
- **acceptance**: Drive pinned real CLI through a disposable sandbox and regression
- **acceptance**: Add a 100-skill multi-source multi-agent performance fixture
- **harness**: Unify App suites and preview on one fake backend
- **update**: Cover stale archive reuse and scoped check state
- **acceptance**: Cover project scope with real CLI, security and perf fixtures
- **scope**: Cover launch checks, inactive lock errors, relocate and wizard scope

### Merge

- Ticket 01 project registry and scope plumbing
- Ticket 02 project read model and lock parsing
- Ticket 03 scope switcher UI
- Ticket 05 project update checks
- Ticket 04 project install and attachments
- Ticket 06 project update and uninstall
- Ticket 07 extra agent project directories
- Ticket 08 acceptance and hardening
- Code review fixes
- Project scope feature

### Wip

- **ticket 03**: Checkpoint partial detail, baseline, and markdown work
- **ticket 04**: Checkpoint partial task and preview work
- **ticket 05**: Checkpoint install task refinements
- **ticket 07**: Checkpoint update UI and backup refinements
- **ticket 10**: Checkpoint permission degradation and executor hardening
- **ticket 11**: Checkpoint acceptance records and sandbox test refinements
- **review-fixes**: Checkpoint P0 and contract fixes
- **review-fixes**: Checkpoint contract and frontend runner work

<!-- generated by git-cliff -->

# Native Plugin: concentrate host semantics

Date: 2026-11-20.
Status: implemented and verified on 2026-11-21.

## Problem and intended result

Codex and Claude native Plugin rules are spread across `inventory.rs`,
`context.rs`, `operations.rs`, `contracts.rs` and parts of `mod.rs`. Reading
configuration ownership, deciding whether an action is allowed, constructing
its command and checking its effects requires following the same host across
those files. The context projection calls `operations::arguments` to derive
page action blocks, coupling observation to command construction.

Deepen the native Plugin host module: each host owns its complete rule chain,
from native evidence to operation verification. Codex and Claude are the two
real adapters at this seam. The common operation module retains confirmation,
mutation ownership, operation records and result classification. Callers gain
leverage from one coherent host interface; host changes gain locality because
their rules and evidence stay together.

## Confirmed scope

The user selected architecture candidate 2 and accepted Q1 on 2026-11-20:
concentrate the complete rule chain for both hosts, including native state
interpretation, configuration ownership, eligibility, commands and write
evidence. A command-only extraction would leave the central ownership problem
unresolved.

Continue the behavior-preserving constraint from candidate 1. Preserve current
observable behavior, error precedence, confirmation conditions, write timing,
capability failure handling and partial/unknown outcomes. Independently found
behavior problems are recorded separately rather than fixed in this refactor.

## Ownership

| Owner | Responsibilities |
| --- | --- |
| Codex host module | Native catalog and configuration interpretation; recursive origin ownership; user configuration version; Project read projection and unavailable writes; supported actions and sources; eligibility; CLI/RPC operation shape; local manifest selection; confirmation evidence and host-specific consequences; response and effect verification |
| Claude host module | Native catalog and installation interpretation; user, Project, local and managed configuration; declaration ownership, inherited installations and enablement; supported actions and sources; eligibility; CLI scope and argument shape; confirmation evidence and host-specific consequences; response and effect verification, including preservation of other configuration layers |
| Common operation module | Resolve registered Scope and Project revision; preview token/expiry; comparison of confirmation evidence; shared mutation permit and worker lifetime; write/verify/finished records; capability failure cache and probe result merging; final outcome classification |
| Transport module | Executable discovery, environment isolation, process ownership and reaping, read timeout, diagnostic redaction, official CLI invocation and RPC framing |
| Shared content helpers | Safe local path resolution, bounded Plugin details, identifiers and hashing, and genuinely common catalog/presentation operations |

Default action/source declarations and each host's compatibility probe sequence
belong with the host rules. The common probe runner retains isolation, execution
ownership and cache merging. Transport can retain mechanical host selection
for executable/environment setup; it does not decide Plugin ownership or
operation semantics.

## Interface and data

Use a small internal host interface covering observation, action preparation,
execution and verification. Concrete Rust names and representation are
implementation choices; a closed dispatch for the existing two hosts is
sufficient. No extensible host registry or new transport abstraction is needed.

- An observation combines the public Snapshot with private host evidence.
  Codex configuration versions/origins and Claude raw configuration layers
  remain inside the host implementation. Public coordination does not inspect
  their JSON fields or construct host-specific evidence.
- Preparation validates a Request against the observation and produces the
  command description, confirmation fingerprint, consequences and affected
  Plugins needed by the existing Preview. Host-only execution details stay
  behind the seam. A prepared value does not authorize skipping fresh
  observation or validation after confirmation.
- Each host uses the same eligibility rules for Snapshot action blocks,
  preview and execution. Reading action blocks must not construct executable
  commands or call back into the common operation module. Keep validation
  ordering and blocked messages intact.
- Execution uses the official host interface and retains the current final
  request validation. Native response evidence remains host-owned. Common
  coordination receives command success/diagnostics and verification facts,
  rather than interpreting native response fields.
- Verification distinguishes response-contract acceptance from observed
  effects. Preserve that distinction when a post-write read fails: an invalid
  successful response can still disable an action, while unavailable
  post-write state cannot be treated as proven failure or success.

Shared rules may remain private helpers used by both host implementations.
They must not recreate a second public coordination protocol or require callers
to sequence individual parsing/projection steps. Separate host files may have
private submodules if that improves navigation; depth is measured by what the
caller must know, not by file size.

Split the host-specific work currently in `inventory.rs` and `context.rs` into
the two host implementations. Retain only justified shared work. Remove the
old context-to-command dependency and obsolete interfaces once callers and
tests migrate; do not retain pass-through wrappers solely to avoid updating
tests. The deletion test is that removing the host module would scatter these
rules and native evidence back across callers, rather than merely removing
indirection.

## Behavior that must survive

1. Native Agent state stays authoritative. There is no app-owned Plugin
   installation ledger, direct configuration-write fallback, automatic retry
   or rollback, or full Plugin backup.
2. Global operations use the user configuration layer. Project operations keep
   their resolved root, revision and explicit native layer. Codex Local remains
   unavailable and current Codex Project writes remain blocked with their
   existing reasons; no write is redirected to Global.
3. Claude keeps declaration-only markets, settings without installations,
   inherited versus selected installations, session-only Plugins, effective
   enablement and managed-policy restrictions. Configuration alone is not
   proof that a Plugin was loaded.
4. Shared-content sync, update and reinstall retain their affected-object
   confirmation. Other layers' enablement/settings and installation identities
   must survive. Cross-layer deletion remains blocked. Codex reinstall enables
   again; Claude update preserves the selected enablement.
5. Preview and execution retain host/context checks, capability gates, source
   and identity validation, ownership/source restrictions, installation checks
   and host write prerequisites in the same order. Multiple simultaneous
   failures must not change which error is returned.
6. Confirmation retains native configuration and catalog evidence, CLI version
   and executable identity, resolved context/revision and local source
   manifest contents. Codex native-versus-legacy manifest selection remains
   unchanged. Relocate or a change after preview requires reconfirmation.
7. The operation worker owns the mutation permit and native process through
   completion and verification. Dropping the caller does not cancel a write;
   normal exit waits. Interrupted operation records remain evidence of an
   unknown result.
8. Result precedence remains: failed post-read or newly blocked action yields
   unconfirmed; accepted response plus valid effects yields succeeded;
   otherwise changed native evidence yields partial, host interaction yields
   needs_host, a failed command yields failed, and the remainder is unconfirmed.
   Keep command success separate from state verification.
9. Unexpected successful responses/effects disable only the affected action
   for the existing CLI identity and configuration-layer cache key. Basic
   isolated probes do not certify Project writes, remote sources, sync or
   cascading removal, and do not reset their cached failures. CLI versions
   remain evidence, not an exact-version allowlist.
10. Catalog/list read degradation preserves current browsing and action blocks.
    Project registry warnings continue to prevent uncertain market removal.
    Relocate and Forget do not migrate or delete native Plugin data.

The existing public IPC model, frontend behavior and serialized operation
records remain compatible. No new domain term is needed. ADRs 0037–0040 remain
in force and are not reopened by this ownership change.

## Implementation and verification plan

1. Establish the existing native Plugin test baseline and map assertions to
   observable contracts. Baseline on 2026-11-20:
   `cargo test --locked --lib native_plugins::` — 29 passed, 0 failed, 9 ignored.
2. Move host evidence and read projection into the host implementations, then
   move eligibility, command/source semantics and verification together.
   Migrate common coordination and compatibility probing to the same interface.
3. Update tests at the host interface and existing public operation interface.
   Use existing fake CLI executables and temporary native configuration trees;
   no test-only public parsing/projection steps. Retain existing behavior
   coverage when relocating assertions and remove obsolete duplicate tests
   only after equivalent coverage exists.
4. Verify both hosts' action eligibility agrees between page state, preview and
   execution, including failure precedence; check ownership and inherited
   state; confirmation races; native response validation and unknown results;
   foreign-layer preservation; cache/probe behavior; detached-worker lifetime
   and interrupted records. Add focused cases only for uncovered contracts
   exposed by the new seam.
5. Run the focused native Plugin suite, then `cargo test --locked`, changed-file
   Rust formatting checks and `git diff --check`. Real CLI/network tests retain
   their existing opt-in policy. Report skipped tests and distinguish any
   pre-existing failures from regressions.

Architecture acceptance: common coordination no longer reads host-specific
configuration/response JSON or branches on Codex/Claude operation semantics;
raw evidence stays with its host; action blocks no longer depend on constructing
commands; adding a host-specific rule does not require changes across inventory,
context, command and result modules. Common transport and lifecycle behavior
remain reusable without acquiring native Plugin rules.

## Final confirmation

The user confirmed the complete ownership split, behavior-preserving scope and
verification plan on 2026-11-20, then requested implementation to continue.

## Implementation results

- `hosts/codex.rs` and `hosts/claude.rs` now own their native observation,
  configuration ownership, eligibility, commands, confirmation evidence,
  consequences, response contracts, effect checks and probe sequences.
- `Host::observe` returns an `Observation` containing a public Snapshot and
  private host evidence. Its `prepare`, `execute` and `verify` methods provide
  the common coordination seam. `CommandResult` exposes command success,
  diagnostics and response acceptance while retaining native JSON privately.
- Page action blocks, preparation and final execution validation share the
  ordered eligibility checks. Page projection no longer builds commands or
  calls the operation coordinator. `context.rs` was removed; `inventory.rs`
  retains content, identity and hashing helpers. The existing public IPC model
  and serialized records are unchanged.
- Host tests moved to `hosts/tests.rs`, allowing raw-evidence assertions to stay
  inside their owner without production test interfaces. Five focused tests
  cover Plugin and marketplace eligibility/error precedence across page,
  preparation and execution; Codex write prerequisites and identity checks;
  response failure caching despite degraded post-read state; and native versus
  legacy local manifest selection for confirmation.

Validation on 2026-11-21:

- Existing native Plugin tests after migration: 29 passed, 0 failed, 9 ignored.
- Final `cargo test --locked --lib native_plugins::`: 34 passed, 0 failed,
  9 ignored.
- Final `cargo test --locked`: 892 passed, 0 failed, 31 ignored across all suites.
- Changed-file Rust formatting, `git diff --check` and new-file whitespace
  checks passed. Common inventory/coordination no longer branches on host
  operation semantics or reads native configuration/response fields; host
  production code has no dependency on `operations`.

Real CLI/network and other opt-in tests retain their existing ignore policy;
the frontend suite was not rerun. No new dependency, native data migration,
direct configuration-write fallback or additional host capability was added.

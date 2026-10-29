# One error taxonomy across the IPC seam

Status: accepted

The backend had 81 string error codes, 78 hand-written constructors, 15 production `AppError::new` bypasses, and three CLI-output classifiers (preview, install/update, uninstall) with duplicated network checks and a duplicated `ExecError` map. The frontend dropped `retryable` in `asAppError` and carried four shadow error interfaces. We replace the string code surface with a typed `ErrorCode` enum (serialized as the same dotted strings), keep the named constructors as the single owner of message/hint/retryability, make `AppError::new` private, and add semantic constructors for the former bypasses. A single `error::cli` module classifies add/remove output into `CliFailure`, maps it to the existing action-specific errors, and owns `ExecError` mapping. The frontend uses one `AppError` shape with an optional code and a required `retryable`; `asAppError` preserves every backend field, client fallbacks omit the code, the four shadow types are deleted, and `isLockError` replaces raw prefix checks. No TypeScript code generation is introduced: the frontend only branches on the lock family.

## Considered options

- **Keep string constants and only add constructors**: rejected because the 81-code vocabulary would remain untyped and an accidental new code string could bypass the registry.
- **A central `meta(code)` message table**: rejected because the constructors already own dynamic, parameterized copy; moving 43 fixed messages and 69 hints into a table would rewrite the taxonomy for no behavioral gain.
- **One `classify_output(action)` function**: rejected because add and remove have different text grammars; one `CliFailure` plus two classifiers and one action mapper keeps the shared result without mixing the grammars.
- **Generated TypeScript code union**: rejected for this change; the frontend only branches on `lock.*`, and a generated list would add build infrastructure without a current consumer.
- **Move env/GitHub errors into the CLI classifier**: rejected because they already have typed error sources and are not CLI-output classification.

## Consequences

- `ErrorCode::ALL` and its registry test lock the 81 dotted strings; `ErrorCode` serializes to the same value the frontend already receives.
- `AppError::new` is private; every code is paired with its copy/hint/retryability in `error/constructors.rs` (the existing named constructors plus semantic bypass constructors).
- Rust control flow uses `ErrorCode` variants (`SkillNotInSource`, `is_env`) instead of string comparisons.
- `error::cli` preserves the existing network/auth/empty/partial/exit mapping per action; preview's duplicate `ExecError` map is gone.
- The frontend keeps `retryable` on every normalized error and no longer invents client-side codes; `isLockError` is the only code-prefix branch.
- Error messages, hints, retryability and CLI outcomes remain unchanged; the only new frontend behavior is that `retryable` is available for a future retry UI.

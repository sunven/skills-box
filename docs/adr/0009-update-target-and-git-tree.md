# One update target and one Git tree implementation

Status: accepted

Update checking and update execution normalized a lock entry's source and ref independently. Only execution validated the ref; checking passed an unsafe ref into cache lookups and GitHub requests. `tasks/conflicts.rs` reached into the mutation planner just to normalize a source for display. Separately, `baseline::git_tree_hash` and `RemoteTree::folder_tree_sha` each implemented Git tree entry ordering, mode strings, SHA-1 object framing and the empty tree, with no test proving the local and remote adapters agree.

We add `src-tauri/src/update_target.rs`: `resolve_source(entry)` owns GitHub source selection from `source` or `sourceUrl`, `resolve(entry)` adds ref normalization and safety validation, and `UpdateTarget::cli_source()` builds `owner/repo[#ref]`. Check mapping keeps its existing notes, execution maps the domain error to `SkillSourceUnresolvable`, and conflicts uses the source-only resolver so stale locks still display a normalized source when their ref is unsafe. The check keeps its existing validation order (source type → hash → skillPath → resolver); unsafe refs become `unknown` with a note instead of a network failure. The existing ref rules are unchanged.

We add `src-tauri/src/git_tree.rs` with typed `GitMode`/`GitTreeEntry`, `object_sha`, `hex`, `build_tree_sha`, `EMPTY_TREE_SHA` and `is_git_tree_hash`. `RemoteTree::folder_tree_sha` becomes a thin adapter from recursive entries; `baseline::git_tree_hash` remains the local walker and keeps installer exclusions, empty-directory skipping and `io::Result` semantics. A cross-adapter test builds the same logical tree locally and as a synthetic `RemoteTree` and requires both to equal a `git write-tree` SHA, including the `a`/`a.b` ordering case.

## Considered options

- **Put the resolver in `update/target.rs`**: rejected because update execution would depend on the read-only update domain; a top-level domain module is shared in both directions.
- **Put the resolver in `tasks/source.rs`**: rejected because install-source validation rejects refs entirely; lock ref rules belong to updates.
- **Return `AppError` from the resolver**: rejected because checking reports uncheckable rows as notes while execution skips with `SkillSourceUnresolvable`; callers keep their presentation.
- **Keep two hash builders and share only a test**: rejected because the duplicate sort/mode/object framing is exactly where the implementations can drift.
- **String modes and SHA strings in the shared builder**: rejected; bytes keep Git ordering and object framing explicit and let adapters own only decoding/encoding.

## Consequences

- Source resolution now accepts public HTTPS Git sources as well as GitHub. `SourceSpec` distinguishes the two: GitHub keeps `owner/repo`, while generic Git keeps an HTTPS clone URL with `.git` so the pinned CLI does not attempt web downloads. Generic lock entries (`git`/`gitlab`) prefer `sourceUrl`, then a complete HTTPS `source`; GitHub entries retain the original source selection. Source identity preserves generic path case. Install still uses the default branch, while updates preserve safe lock refs.

- Check and execution resolve the same source/ref; unsafe refs no longer trigger GitHub requests during checking.
- `UpdateItem.lock_source` and `UpdateItem.reference` are removed; only the CLI source string remains.
- Local and remote tree hashes are unchanged and now share one implementation and one fixed-SHA contract test.
- `update` no longer imports `baseline` for the Git hash-format predicate.
- Error codes/messages, IPC, UI and the check/execution split are unchanged.

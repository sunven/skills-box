# Rust owns command previews and actual invocation records

Status: accepted

Every operation that invokes `npx skills` shows its command. Rust derives
manual previews from the existing validated action plans and `ScopedCli`
CommandSpec, formatting `cd <actual cwd> && <absolute npx path> …` with shell
quoting. Frontend reconstruction was rejected because source normalization,
Scope, selected Agents and runtime discovery can change the actual argv or cwd.

Install, Update and Uninstall starts revalidate the preview fingerprint under
the Mutation permit before task registration; sessions compare the actual
command again before invocation. Batch operations retain their existing
preview/revision contract and additionally compare the full commands at start
and per target. A mismatch requires a refreshed preview and another click;
retry retains only the selected failed or unexecuted targets.

One observed ProcessExecutor publishes session-only command records for
Mutation tasks, read-only source previews, environment version probes and
discovery catalog calls. Records begin at invocation and settle with the CLI
outcome, independently of later filesystem verification. Tasks retain their
calls; background contexts retain their latest actual call with its start time.
Native actions, cache hits and targets skipped or cancelled before invocation
have no call. Automatic background work keeps its existing lifecycle, and
records are never persisted as a second operation history.

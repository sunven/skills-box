# Scope mutations own blocking work through completion

Status: accepted

Install, Update and Uninstall kept the write permit in the requesting async
future while blocking planning held only a cloned Scope layout. Dropping the
request released the permit even though Tokio cannot abort a blocking worker
that has already started. TaskManager's internal scoped-mutation module now
owns acquisition before Scope resolution, carries permit and layout together
into planning, and returns an owned plan that consumes both through registration
and session completion. Native Attach and Detach use the same acquisition
sequence and retain ownership in their blocking workers. A `(permit, layout)`
pair returned to callers was rejected because it leaves the lifetime protocol
in each caller.

Planning remains outside the task registry and creates no interruption marker
until validation succeeds. Registration checks shutdown under the same task-map
lock used to observe active tasks: shutdown either sees the registered Mutation
task and cancels it, or prevents it from registering. A planning result cannot
start a new Mutation task after shutdown has begun. Planning errors, worker
panics and dropped requests release the slot when their actual work finishes.

This preserves ADRs 0003 and 0013: TaskManager owns permit, registry and marker;
the session owns execution. IPC, task events/results, cancellation windows and
error precedence remain unchanged, including Install's busy-before-invalid-
request and Update's pre-permit name validation. Preview, reads, Update checks
and Extra agent configuration retain their existing lifecycles.

The internal planning callback is the seam used by the three real planners and
by deterministic paused tests. It is private to TaskManager, rather than a new
public planner adapter. Tests in
`src-tauri/src/tasks/manager/scoped_mutation/tests.rs` exercise ownership,
shutdown, refusal order and release; existing task/action tests lock execution
and marker behavior.

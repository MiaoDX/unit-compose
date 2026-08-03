# Execution semantics — superseded

- **Status:** Superseded for V0
- **Current contract:** [UnitCompose V0 architecture specification](v0-architecture.md)

The former execution specification described provisional publications, Run-level commit, persistent Resource successors, storage leases, and poisoned failure behavior.

V0 instead uses a smaller contract:

- static Resource DAG;
- stable sequential topological execution;
- complete output publication per Unit;
- stop on the first Unit error;
- no framework rollback of Unit private state or external effects;
- build-new-and-swap configuration reload between runs.

The earlier text remains available in Git history and the transaction research notes.

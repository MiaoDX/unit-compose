# Alpha scope — superseded

- **Status:** Superseded for V0
- **Current scope:** [UnitCompose V0 architecture specification](v0-architecture.md)

The earlier Alpha scope attempted to validate transactional commit, staged persistent updates, poisoned Module behavior, leases, and storage identity in the first implementation.

The accepted V0 scope now focuses on:

- Unit Registry and YAML Module Definitions;
- typed Unit ports and named Resources;
- static DAG validation;
- stable sequential execution;
- configuration-based Unit replacement;
- read-only Debug, graph export, timing, and diagnostics;
- host embedding.

Transactional state, generalized storage management, automatic parallelism, dynamic plugins, Python, and recovery remain deferred.

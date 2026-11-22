# Concurrency status

`z200c` now has the first layer of process concurrency.

## Implemented

- `PROCESS (params)` definitions.
- `START process(args);` actions.
- `STOP;` actions.
- Processes run under a cooperative round-robin scheduler.
- The scheduler yields at action boundaries, including inside procedure and process
  bodies.
- Process parameters currently use `IN` passing.
- Module-level locations are shared between process activations (the same cells are
  visible to all process interpreters).
- Process-local declarations and parameters are per activation.
- Exceptions raised in a process are reported through the normal interpreter error
  path.
- `processes.ch` covers basic process startup, shared state, `STOP`, and process-local
  assertions.

## Not implemented yet

- `DELAY` and timer/event locations.
- signals, `SEND`, receive cases.
- region/task mutual exclusion and critical procedures.
- process handlers and full exception termination semantics.
- process instance values and `THIS`.
- `LOC` and `OUT`/`INOUT` process parameters.
- fair/priority scheduling guarantees.
- process definitions outside the module declaration list.

The scheduler is intentionally an execution foundation, not yet a complete
implementation of Z.200 clause 11.

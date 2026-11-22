# Concurrency status

`z200c` now has the first layer of process concurrency.

## Implemented

- `PROCESS (params)` definitions.
- `START process(args);` actions.
- `STOP;` actions.
- `EVENT` and `EVENT(n)` synchronization modes.
- `DELAY event [PRIORITY expr];` and `CONTINUE event;`.
- `SIGNAL name [= (modes)];` declarations.
- `SEND signal [(values)];`.
- `RECEIVE CASE (signal [IN names]): actions ... [ELSE actions] ESAC;`.
- Pending signals are queued; waiting receivers are parked on every alternative and
  woken when a matching signal arrives.
- Signal values are copied into the receive names before the branch executes.
- Region mutual exclusion: calls to procedures defined inside a `REGION` acquire a
  per-region lock. A second process attempting a region call is queued until the
  current owner releases the region.
- Region locks are reentrant for the owning process and queued calls resume FIFO.
- `DURATION` values and `MILLISECS`, `SECS`, `MINUTES`, `HOURS`, `DAYS` built-ins.
- `DELAY CASE (event, ...): actions ... ESAC`, including priority and timeout
  cancellation when nested in a relative timing action.
- Relative timing actions:
  `AFTER duration [DELAY] IN actions TIMEOUT actions END`.
- Timers are checked when a process blocks in `DELAY` or `RECEIVE CASE`; expiry
  aborts that wait and runs the `TIMEOUT` handler.
- Events wake the highest-priority delayed process; equal priorities use FIFO order.
- Event lengths are enforced and raise `DELAYFAIL`.
- A scheduler run with only delayed processes reports a deadlock.
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

- `AT` absolute timing actions and `CYCLE` periodic timing actions.
- timeout supervision for region calls.
- `SEND ... TO` process targeting and process instance values.
- `RECEIVE CASE SET` instance locations.
- signal priorities.
- buffer modes and buffer send/receive.
- region locations (region object modes), task modes, and critical procedures on
  region objects.
- process handlers and full exception termination semantics.
- process instance values and `THIS`.
- `LOC` and `OUT`/`INOUT` process parameters.
- fair/priority scheduling guarantees.
- process definitions outside the module declaration list.

The scheduler is intentionally an execution foundation, not yet a complete
implementation of Z.200 clause 11.

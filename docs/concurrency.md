# Concurrency status

`z200c` implements a single-threaded cooperative concurrency runtime.

## Implemented

- `PROCESS (params)` definitions with `IN` and `LOC` parameters.
- `START` as both an action and an expression returning an `INSTANCE`.
- `STOP`, `THIS`, and `INSTANCE` values.
- Cooperative round-robin scheduling, yielding at action boundaries.
- Shared module-level locations across process activations.
- Per-process locals, parameters, and interpreter state.
- Process `ON` handlers.
- `EVENT` and `EVENT(n)` modes.
- `DELAY event [PRIORITY expr];`, `DELAY CASE`, and `CONTINUE event;`.
- Delay-case alternatives can wait on multiple events; continuation wakes the
  highest-priority waiter, FIFO among ties.
- `SIGNAL name [= (modes)] [TO process-name];`.
- `SEND signal [(values)] [TO instance] [PRIORITY expr];`.
- `RECEIVE CASE [SET instance;] ... [ELSE ...] ESAC`.
- Targeted signals, pending targeted messages, sender instance capture, and
  process-name restrictions.
- `BUFFER [(length)] element-mode`.
- Buffered send/receive, capacity blocking, delayed senders, FIFO ordering among
  equal priorities, and send priorities.
- Region module mutual exclusion for calls to procedures defined in a `REGION`.
- Region object modes with per-object mutually exclusive component method calls.
- Task object modes with a per-object task worker:
  - async queued component procedure calls,
  - task procedures restricted to `IN` parameters and no result,
  - direct field access serialized against task methods,
  - `TERMINATE` stopping an allocated task worker.
- `DURATION`, `TIME`, `MILLISECS`, `SECS`, `MINUTES`, `HOURS`, `DAYS`, and `ABSTIME`.
- Relative timing: `AFTER duration [DELAY] IN actions TIMEOUT actions END`.
- Absolute timing: `AT time IN actions TIMEOUT actions END`.
- Cyclic timing: `CYCLE duration IN actions END`.
- Scheduler deadlines, timeout cancellation of blocking waits, and a non-busy-wait
  sleep path when only timed waiters remain.
- Grant encoding preserves module, region, and task object mode kinds.

## End-to-end example

`examples/pipeline` is the concurrency integration application. It combines
bounded buffers, producers/workers/collector processes, region-object statistics,
signals, targeted sends, receive `SET`, relative timing, and a watchdog, then
asserts end-to-end delivery invariants.

## Current limitations

- The main module body pumps one ready process at action boundaries, so spawned
  processes interleave with main-process actions.
- Main-process `DELAY`/timed `DELAY` and `RECEIVE CASE` block by pumping the scheduler
  until a continuation, signal, buffer message, or timeout wakes the main process.
- Full coroutine-based main-process termination semantics remain deferred.
- Timeouts are enforced at blocking points (`DELAY`, `RECEIVE`, buffer waits), not
  during arbitrary long-running computation.
- Region-call timeout supervision is not implemented.
- Task-object workers are stopped by `TERMINATE` or when a statically declared task
  object's enclosing scope ends.
- Buffer grant encoding supports referable element modes, including structure modes.
- Signal `TO process-name` matching uses the process definition name; process
  instance values do not carry separate dynamic names.
- Scheduler fairness is simple round-robin; no priority scheduling guarantees.
- Process definitions outside the module declaration list are not supported.
- Generic process and region templates are supported; template instances expand before
  semantic analysis.
## Scheduler stress testing

`Z200C_YIELD_INTERVAL=n` changes the cooperative yield cadence for testing. The default
is `1` (yield at every action). The CLI integration suite runs `examples/pipeline` with
several intervals to exercise different interleavings while asserting the same
end-to-end invariants.

The scheduler is an execution foundation for the supported subset, not yet a complete
implementation of Z.200 clause 11.

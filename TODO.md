# TODO

Prioritized work after the reach/context/piecewise work. Items are ordered; each should
land as a small, green change.

## 1. Unify the reach model

- [x] Introduce a shared export/reach representation (`src/reach.rs`) used by `sema`
      and `interp` (selection rules, not yet the full reach tree).
- [x] Move prefix matching, `TO` friend checks, and ambiguity rules into shared helpers.
- [x] Replace the duplicated `ReachExport` types in `sema.rs` and `interp.rs` with the
      shared metadata plus checker/interpreter-specific payloads.
- [x] Keep one `SEIZE`/`SEIZE ALL` selection path so checker and interpreter cannot
      diverge.
- [x] Add focused tests for prefix/friend/ambiguity cases.
- [x] Shared static `ReachTree` for module declarations and modes, consumed by
      `program`, `sema`, and `interp`.
- [x] Shared child/hoisting plan (`children`, `hoisted`), used by the interpreter's
      module-body hoisting.
- [x] Shared module-reach exports (`exports` with owner, kind, prefix/friend metadata),
      used to seed checker and interpreter module scopes.
- [x] Top-level `GRANT` child-export collection now uses the tree; removed the separate
      `collect_child_grants`/`collect_module_exports` path.
- [x] Block-level reach exports (`BEGIN`/`IF`/`DO`/`CASE`) are tree nodes keyed by
      block/branch/alternative spans and seed both checker and interpreter scopes.
- [x] Nested `GRANT` validation (duplicate/unknown names, unsupported old prefixes) and
      `FORBID` application moved into reach-tree construction; removed the checker's
      `collect_module_grants` and the interpreter's runtime grant registration.
- [x] Interpreter hoisting uses one tree-driven path for module bodies and block reaches;
      removed IR statement scanning for module declarations.

Acceptance for completion: no duplicate reach-matching or reach-building logic remains;
all existing tests pass with no new special cases.

## 2. Move virtual grants into analysis

- [x] Replace the loader-side `apply_virtual_grants` mutation with a normalization step
      (`program::normalize`) used by semantic entry points.
- [x] Ensure `run_source`/direct `check`/`grant` see the same virtual grants as the CLI
      loader path.
- [x] Add regression tests for direct entry points and normalization idempotence.
- [x] Remove the loader-side mutation.

## 3. Conformance and negative fixtures

- [x] Build a starter fixture set for the implemented subset (units, contexts,
      nested specs, remote syntax, nested/block reaches, spec regions, virtual grants).
- [x] Add negative fixtures for region actions, first-context visibility, unnamed
      nested specs, nested grant ambiguity, nested `FORBID`, `TO` restrictions, and
      nested/region interface mismatches.
- [x] Add CLI structural assertions for grant-file output (`schema = 4`, no
      `int_widths`/`edition`) and existing diagnostic-code assertions.
- [x] Encode named set modes, reference/pointer/row modes, buffer element modes, and
      module-mode types at grant-interface boundaries.
- [x] Expand fixtures with direct Z.200 examples (expressions, control flow,
      procedures, modes/structures, visibility, arrays, handlers) and a `run_fail`
      suite for runtime checks (`ASSERTFAIL`, `OVERFLOW`, array bounds, undefined
      values, `TEXTFAIL`, unhandled exceptions). Full stdout snapshots were not added;
      the interpreter writes directly to stdout, and the `ASSERT`-based fixtures already
      assert observable behavior.
- [x] Extend regression coverage for newer features: generic actual-kind mismatch,
      powerset member mismatch, float division by zero, negative variable widths, and
      a golden stdout snapshot for `WRITE`/`WRITELINE` through the CLI.

## 4. Decide product direction (resolved: interpreter-first)

- [x] Choose interpreter-first educational compiler vs native/C backend.
      Decision: interpreter-first educational compiler.
- [x] Minimal C backend and `z200c compile`: basic variables, expressions,
      assignments, `ASSERT`, `IF`, `WRITE`/`WRITELINE`, procedures with `IN`
      parameters (including `OUT`/`INOUT`/`LOC`), calls, `RETURN`, `RESULT`, integer
      conversions, one-dimensional
      scalar arrays (including multidimensional), array initializers/literals,
      same-mode array copying and comparison, string comparisons and concatenation,
      named set modes, scalar references (including array elements) and `NULL`, loop actions,
      multi-selector `CASE`, and scoped `BEGIN` blocks.
- [ ] Extend native codegen to procedures, calls, arrays, text, references, exceptions,
      and a dedicated CHILL runtime ABI.
- [-] Full native concurrency and moreta object lowering remain deferred.
- [x] Direction: prioritize language coverage/conformance; start with strings, then
      arrays, exceptions/handlers, and I/O.

## 5. CI

- [x] CI workflow runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      and `cargo test --all` on stable Linux.
- [x] `rust-toolchain.toml` pins the stable channel with rustfmt/clippy.
- [ ] Decide whether to declare and test an MSRV; current policy is stable-only.
- [ ] Optionally add a stable/nightly matrix once there is a reason.

## 6. Concurrency (post-first-release)

First release policy: **language coverage, not faithful concurrency**. Regions and
processes are single-threaded/non-preemptive for now; faithful CHILL concurrency is
deferred.

- [x] Decide first-release policy: keep execution single-threaded.
- [x] Design and implement cooperative region mutual exclusion and per-object
      serialization for region methods.
- [x] Implement cooperative process scheduling, signals, buffers, timers, and
      task-object workers.
- [x] Support nested regions inside module bodies.

## 7. Core language coverage

- [x] One-dimensional scalar arrays (`ARRAY (low:high) mode`) with indexing, bounds
      checks, and whole-array assignment.
- [x] Arrays of structures, including field selection and field assignment through
      array elements.
- [x] Unlabelled array literals/tuples (`(: v, ... :)` and `[v, ...]`) in contexts
      with a known array mode.
- [x] Array slices (`a(lower:upper)` and `a(first UP size)`), usable as array values
      and assignment targets, with constant or runtime bounds.
- [x] Multidimensional arrays up to four dimensions, with multi-index access,
      whole-array assignment/equality, and scalar or structure elements.
- [x] Labelled array tuples with literal/range/`ELSE`/`(*)` labels, completeness and
      overlap checking.
- [x] `CHAR`, unbounded `CHARS`, fixed `CHARS(n)`, and bounded
      `CHARS(n) VARYING` modes.
- [x] `WCHAR` and `WCHARS` scalar/string modes, wide character and string literals,
      bounded `WCHARS(n)` modes, wide-string concatenation/comparison, wide powersets,
      and grant-interface encoding.
- [x] Bit strings: `BOOLS`, bounded `BOOLS(n)` modes, binary/octal/hexadecimal bit
      literals, elementwise logic, concatenation, comparison, and grant encoding.
- [x] References and rows: `REF M`, `PTR`, `ROW M`, `NULL`, referencing/dereferencing,
      `ALLOCATE`/`GETSTACK`/`TERMINATE`, `EMPTY`, self-referential structure modes, and
      grant encoding for reference/pointer/row modes. Arrays of references, nested
      references, and references to fields/indexes remain deferred.
- [x] `TEXT(n)` and `WTEXT(n)` text location modes with empty-string initialization,
      text I/O, bounded length checks, optional index-mode/DYNAMIC surface syntax,
      grant encoding, actual text index, and `GETTEXTINDEX`/`SETTEXTINDEX`. Text
      sub-location reference built-ins remain deferred.
- [x] `CAUSE` and module/procedure handlers, including catching `ASSERTFAIL`/`OVERFLOW`.
- [x] Action-level handlers, including handlers on bracketed actions, assignment,
      assert, and call actions.
- [x] Exception-set flow checking for `CAUSE`, `ASSERT`, and called procedures;
      `EXCEPTIONS` lists are enforced and compared across spec/body signatures.
      Implicit runtime checks such as `OVERFLOW`, range, and text failures are not
      statically inferred yet.
- [x] Multiple `CASE` selectors with per-selector label lists, including `BOOL` and
      `CHAR` selectors, labels, and character ranges.
- [x] Implementation-defined text I/O (`WRITE`, `WRITELINE`, `READLINE`).
- [x] `READTEXT`/`WRITETEXT` for `CHARS` locations with literal format text.
- [x] Bounded text modes (`CHARS(n)` and `CHARS(n) VARYING`), including arrays of
      bounded strings.
- [x] `READTEXT`/`WRITETEXT` format conversion and editing codes with io lists
      (`%B`, `%O`, `%H`, `%C`, widths, `L`/`E`/`P` qualifiers, repetition, `X`, `>`,
      `<`, `T`, and `%%`).
- [x] Basic `ASSOCIATION` and `ACCESS CHARS` modes with `ASSOCIATE`, `DISSOCIATE`,
      `ISASSOCIATED`, `CREATE`, `DELETE`, `CONNECT`, `DISCONNECT`, `READRECORD`, and
      `WRITERECORD` over sequential line records.
- [x] Parenthesized text-format clauses with repetition.
- [x] Variable format widths (`V`) for conversions and editing clauses.
- [x] Association attributes `EXISTING`, `READABLE`, `WRITEABLE`, `INDEXABLE`,
      `SEQUENCIBLE`, and `VARIABLE`.
- [x] Floating-point mode `FLOAT`, literals, arithmetic/comparisons, `FLOAT(...)`
      conversion, float arrays, fractional/exponent format widths, and `%F`.
- [x] I/O control format clauses (`%/`, `%+`, `%-`, `%?`, `%!`, `%=`).
- [x] Association `MODIFY` (no parameters) and indexed `ACCESS (range) CHARS`,
      indexed `CONNECT`/`READRECORD`/`WRITERECORD`, record-range checks, and
      `INDEXABLE` reporting.
- [x] Predefined `USAGE`/`WHERE` values, `CONNECT` usage/where/index forms,
      `GETUSAGE`, `OUTOFFILE`, `GETASSOCIATION`, usage enforcement, and EOF reporting.
- [x] Text-location access sub-locations: `CONNECT`/`DISCONNECT` on `TEXT`/`WTEXT`,
      file-backed sequential `READTEXT`/`WRITETEXT`, and access attributes for text.
- [x] Reference-backed text record/access sub-locations:
      `GETTEXTRECORD`/`SETTEXTRECORD`, `GETTEXTACCESS`/`SETTEXTACCESS`, and
      dereferenced access references as transfer locations.
- [x] Indexed text access: retained text index modes, indexed text `CONNECT`, indexed
      `READTEXT`/`WRITETEXT`, range checking, and grant encoding.
- [x] Bounded text record cells, length-safe record references, and dereferenced
      `SETTEXTRECORD` sources.
- [x] Processes, signals, buffers, timers, and event selection under the cooperative
      single-threaded scheduler. Faithful preemptive concurrency and main-process
      interleaving remain deferred.
- [x] `POWERSET` of `INT` / `BOOL` / `CHAR` with literals, ranges, union,
      intersection, difference, equality/ordering, and `IN`.
- [x] Named `SET (...)` modes with unnumbered and numbered elements.
- [x] Generic procedure templates parameterized by `MODE`, with
      `name: PROC = NEW template (modes) END;` instantiation.
- [x] Generic module templates parameterized by `MODE`, with
      `name: MODULE = NEW template (modes) END;` instantiation.
- [x] Moreta module-body objects with variable/procedure components, value-returning
      methods, non-value method actions, component `INIT` evaluation (including base
      initializers), `BASED_ON` inheritance/override, paired `MODULE SPEC`
      interfaces, and invariants.
- [x] Consolidate the moreta subset: document semantics/limits, reject object
      assignment, reject abstract-mode instantiation, check invariants after
      initialization, merge spec/body invariants, and key moreta metadata by stable
      mode ID.
- [x] Generic `SYN` value parameters and generic `PROC` parameters in procedure and
      module templates.
- [x] `OUT`, `INOUT`, and `LOC` parameter passing for procedures and moreta
      methods, including copy-back and grant-interface encoding.
- [x] Moreta `SELF`, constructors/destructors, constructor arguments, nested mode
      components, and moreta grant-mode encoding. Task/region object modes and generic
      region/process templates remain deferred.
- [x] Static-object destructors at reach/lifetime end, including block, procedure,
      process, nested-module, and program-end cleanup.
- [x] Base constructors/destructors are excluded from `BASED_ON` inheritance.
- [x] Assignment through fields of multidimensional array elements.
- [x] Wide `ACCESS WCHARS` records, including grant encoding and indexed forms.
- [x] Array elements for sets, powersets, associations, access modes, module-mode
      objects, `REF`, `ROW`, and `PTR`, plus arrays of powerset modes.
- [x] References to fields and array elements, including multidimensional elements.
- [x] Forward-declared, mutually recursive structure modes through `REF`/`ROW`, with
      direct value-cycle rejection.
- [x] References to whole array targets with indexed dereference (`ref->(i)`).
- [x] Nested multidimensional array literals.
- [x] Integration showcase example spanning objects, references, arrays, slices, and
      control flow.
- [x] Full-dimensional constant-bound multidimensional slices, including `UP` size
      and rectangular slice assignment.
- [x] Main-process interleaving at action boundaries via scheduler pumping.
- [x] Main-process blocking `DELAY`, timed `DELAY`, and signal/buffer `RECEIVE CASE`
      through scheduler pumping.
- [x] Deterministic program shutdown: drain processes, stop task workers, run static
      destructors, clear scheduler state, and report residual region/object/buffer waits.
- [ ] Full coroutine-based main-process termination semantics.
- [x] Complete `Mode!Component` path preservation in mode symbols and grant files.
- [x] First concurrency layer: `PROCESS`, `START`, `STOP`, a cooperative
      action-boundary scheduler, shared module locations, and process-local state.
- [x] `EVENT`/`EVENT(n)` modes, `DELAY`, `CONTINUE`, priority wake-up, event-length
      `DELAYFAIL`, and delayed-process deadlock detection.
- [x] `SIGNAL` declarations, `SEND`, `RECEIVE CASE`, pending signal queues, parked
      receivers, receive bindings, and `ELSE` receive alternatives.
- [x] Region-call mutual exclusion for procedures defined inside `REGION` units,
      including queued callers and per-process reentrancy.
- [x] Fixed control-flow continuation so actions after `IF`, `DO`, `CASE`, `BEGIN`,
      labelled actions, and handled actions are executed.
- [x] `DURATION` values, duration built-ins, relative `AFTER ... [DELAY] IN ...
      TIMEOUT ... END` timing actions, scheduler deadlines, and timeout cancellation of
      blocking `DELAY`/`RECEIVE` waits.
- [x] `DELAY CASE` with multiple event alternatives, priority selection, and timeout
      cancellation inside relative timing actions.
- [x] `BUFFER [(length)] element-mode` locations, buffered send/receive, capacity
      blocking, delayed senders, buffer receive timeouts, and grant encoding for
      referable buffer element modes.
- [x] `INSTANCE` values, `THIS`, `START` as an expression, and targeted
      `SEND ... TO instance` delivery with pending targeted signals.
- [x] `RECEIVE CASE SET instance;` for signal and buffer receives, carrying sender
      instance values through queued and direct handoffs.
- [x] Signal `SEND` priority selection and prioritized buffer messages/delayed
      senders.
- [x] Process `ON` handlers for process-local exceptions and `CAUSE`.
- [x] `SIGNAL ... TO process-name` restrictions, enforced for targeted sends and
      receiver matching.
- [x] Process `LOC` parameters, with process actual locations lowered through the
      shared call-argument path; `OUT`/`INOUT` process parameters are rejected.
- [x] Absolute timing actions (`AT`), cyclic timing actions (`CYCLE`), `TIME`, and
      `ABSTIME` construction.
- [x] Region object modes with Moreta components and per-object method-call mutual
      exclusion; region/task object grant-kind preservation.
- [x] `examples/pipeline`: end-to-end concurrent pipeline exercising bounded buffers,
      processes, region objects, signals, targeted sends, receive `SET`, timing, and
      FIFO/priority message ordering.
- [x] Task object modes with per-object task workers, asynchronous queued component
      calls, task-only `IN` procedure validation, and grant-kind preservation.
- [x] Mutually exclusive direct task-object field access and `TERMINATE`-based task
      worker termination.
- [x] Scheduler yield-interval testing and pipeline runs across multiple
      interleavings.

- [x] Single-file script mode: `z200c check <file>.ch` and `z200c run <file>.ch`
      without a manifest, loading only the named script.

## 8. Cleanup

- [x] Replace message-based error classification with explicit codes at checker and
      interpreter error-construction sites.
- [x] Reduce repeated temp-dir setup in CLI tests with a `TempDir` fixture that
      creates sources and cleans up on drop.
- [x] Reconcile docs status matrices with the implementation (subset scope,
      grant/contexts/piecewise notes, text I/O and exception decisions).
